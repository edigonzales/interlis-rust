use super::{
    Analyzer, BoundName, Kind, SyntaxDecl, TypeInfo,
    expressions::{ExpressionContext, scalar_type, underlying_expression},
};
use crate::{
    BoundExpressionKind, BoundPathStep, Cardinality, CollectionKind, CollectionType, ExpressionId,
    ExpressionType, LanguageVersion, NumericBounds, NumericRange, ObjectTarget, Span,
    StructureType, UnitConstant, ViewFormation, ViewInfo,
    syntax::{
        AggregationGrouping, Expression, Property, RenamedViewable, ViewAttribute, ViewOrigin,
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct ViewAttributeDependencies {
    attributes: Vec<(usize, Span)>,
    bases: BTreeSet<usize>,
    nullable: bool,
}

impl Analyzer<'_, '_> {
    pub(super) fn bind_views(
        &mut self,
        documents: &BTreeMap<u32, &[Expression]>,
    ) -> Option<Vec<usize>> {
        let mut pending = BTreeMap::new();
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::View(view)) = self.symbols[id].ast else {
                continue;
            };
            let mut bases = Vec::new();
            let inherits = self.symbols[id].base_scope.is_some();
            let formation = match &view.origin {
                Some(ViewOrigin::Formation(formation)) if inherits => {
                    self.diagnostics.push(
                        "ILI-VIEW-FORMATION-OVERRIDE",
                        "An extension cannot change the inherited formation",
                        Some(formation.span.clone()),
                    );
                    return None;
                }
                Some(ViewOrigin::Formation(formation)) => {
                    let kind = match &formation.value {
                        crate::syntax::ViewFormation::Projection(base) => {
                            bases.push(self.bind_view_base(id, base, false)?);
                            ViewFormation::Projection
                        }
                        crate::syntax::ViewFormation::Join { first, others } => {
                            bases.push(self.bind_view_base(id, first, false)?);
                            for base in others {
                                bases.push(self.bind_view_base(
                                    id,
                                    &base.base,
                                    base.or_null.is_some(),
                                )?);
                            }
                            ViewFormation::Join
                        }
                        crate::syntax::ViewFormation::Union(sources) => {
                            for base in sources {
                                bases.push(self.bind_view_base(id, base, false)?);
                            }
                            ViewFormation::Union
                        }
                        crate::syntax::ViewFormation::Aggregation { base, .. } => {
                            bases.push(self.bind_view_base(id, base, false)?);
                            ViewFormation::Aggregation
                        }
                        _ => unreachable!("unsupported view formations are gated before binding"),
                    };
                    Some(kind)
                }
                _ if inherits => None,
                _ => {
                    self.diagnostics.push(
                        "ILI-VIEW-FORMATION",
                        "A view requires a formation or an inherited view",
                        Some(view.name.span.clone()),
                    );
                    return None;
                }
            };
            for extension in &view.base_extensions {
                for candidate in &extension.extensions {
                    bases.push(self.bind_view_base(id, candidate, true)?);
                }
            }
            pending.insert(id, (formation, bases));
        }
        // Formations, inheritance and extensions all depend on the members of their sources.
        let mut state = vec![0; self.symbols.len()];
        let mut stack = Vec::new();
        let views: Vec<_> = pending.keys().copied().collect();
        for &id in &views {
            if state[id] != 0 {
                continue;
            }
            stack.push((id, 0));
            state[id] = 1;
            while let Some((view, next)) = stack.last_mut() {
                let at = self.symbols[*view].name_span.clone();
                if !self.step(1, &at) {
                    return None;
                }
                let declared = &pending[view].1;
                let source = if let Some(&base) = declared.get(*next) {
                    let base = self.symbols[base].declaration.reference.as_ref()?;
                    Some((self.names[base.target_name()], base))
                } else if *next == declared.len() {
                    self.symbols[*view]
                        .base_scope
                        .zip(self.symbols[*view].declaration.base_reference())
                } else {
                    None
                };
                let Some((source, base)) = source else {
                    let (formation, bases) = pending.remove(view)?;
                    self.complete_view_formation(*view, formation, bases)?;
                    self.bind_view_members(*view, documents)?;
                    state[*view] = 2;
                    stack.pop();
                    continue;
                };
                *next += 1;
                if self.symbols[source].declaration.kind != Kind::View {
                    continue;
                }
                match state[source] {
                    0 => {
                        state[source] = 1;
                        stack.push((source, 0));
                    }
                    1 => {
                        self.diagnostics.push(
                            "ILI-VIEW-CYCLE",
                            "This basis closes a cycle of view formations",
                            Some(base.source_span().clone()),
                        );
                        return None;
                    }
                    _ => {}
                }
            }
        }
        Some(views)
    }

    fn complete_view_formation(
        &mut self,
        id: usize,
        formation: Option<ViewFormation>,
        local: Vec<usize>,
    ) -> Option<()> {
        let Some(SyntaxDecl::View(syntax)) = self.symbols[id].ast else {
            unreachable!("view order contains only views");
        };
        let mut bases = Vec::new();
        let mut selections = Vec::new();
        let mut grouping = Vec::new();
        let formation = if let Some(parent) = self.symbols[id].base_scope {
            let inherited = self.symbols[parent].declaration.view()?;
            if !self.step(
                inherited.bases.len() + inherited.selections.len() + inherited.grouping.len(),
                &syntax.name.span,
            ) {
                return None;
            }
            let inherited = self.symbols[parent].declaration.view()?;
            bases.extend_from_slice(&inherited.bases);
            selections.extend_from_slice(&inherited.selections);
            grouping.extend_from_slice(&inherited.grouping);
            let formation = inherited.formation;
            let at = self.symbols[id]
                .declaration
                .base_reference()?
                .source_span()
                .clone();
            for &basis in &bases {
                let source = self.names[self.symbols[basis]
                    .declaration
                    .type_reference()?
                    .target_name()];
                self.require_view_dependency(id, source, &at)?;
            }
            formation
        } else {
            formation?
        };
        for &basis in &local {
            let decl = &self.symbols[basis].declaration;
            let at = self.symbols[basis].name_span.clone();
            if let Some(parent) = self.symbols[id].base_scope {
                let name = decl.name.clone();
                if self
                    .visible_name(parent, &name, Some(&[Kind::ViewBase, Kind::Attribute]), &at)
                    .ok()?
                    .is_some()
                {
                    self.diagnostics.push(
                        "ILI-VIEW-ALIAS-CONFLICT",
                        "A local basis cannot hide an inherited basis or attribute",
                        Some(at),
                    );
                    return None;
                }
            }
        }
        let count: usize = syntax
            .base_extensions
            .iter()
            .map(|extension| extension.extensions.len())
            .sum();
        let (formation_bases, extension_bases) = local.split_at(local.len() - count);
        bases.extend_from_slice(formation_bases);
        let mut candidates = extension_bases.iter().copied();
        let mut available: BTreeSet<_> = bases.iter().copied().collect();
        for group in &syntax.base_extensions {
            let name = &group.base;
            let Some(original) = self
                .visible_name(id, &name.value, Some(&[Kind::ViewBase]), &name.span)
                .ok()?
                .filter(|original| available.contains(original))
            else {
                self.diagnostics.push(
                    "ILI-VIEW-BASE-EXTENSION",
                    "BASE must name an available basis of the view",
                    Some(name.span.clone()),
                );
                return None;
            };
            let ancestor = self.names[self.symbols[original]
                .declaration
                .reference
                .as_ref()?
                .target_name()];
            for _ in &group.extensions {
                let candidate = candidates.next()?;
                let reference = self.symbols[candidate].declaration.reference.as_ref()?;
                let scope = self.names[reference.target_name()];
                let at = reference.source_span().clone();
                if !self.specializes_symbol(scope, ancestor, &at)? {
                    self.diagnostics.push(
                        "ILI-VIEW-BASE-EXTENSION",
                        "A basis extension must specialize the named basis",
                        Some(at),
                    );
                    return None;
                }
                self.symbols[candidate].declaration.base_reference =
                    Some(self.reference_to_symbol(candidate, original, name)?);
                available.insert(candidate);
                bases.push(candidate);
            }
        }
        self.symbols[id].declaration.view = Some(Box::new(ViewInfo {
            formation,
            bases,
            selections,
            grouping,
        }));
        if let Some(ViewOrigin::Formation(formation)) = &syntax.origin
            && let crate::syntax::ViewFormation::Aggregation { grouping, .. } = &formation.value
        {
            self.bind_aggregation(id, grouping, &formation.span)?;
        }
        Some(())
    }

    fn bind_aggregation(
        &mut self,
        owner: usize,
        grouping: &AggregationGrouping,
        at: &Span,
    ) -> Option<()> {
        let mut keys = Vec::new();
        if let AggregationGrouping::Equal(paths) = grouping {
            for path in &paths.value {
                let (kind, value_type, ty) = self.expression_path(owner, path)?;
                keys.push(self.record_expression(kind, value_type, ty, &path.span));
            }
        }
        let decl = &mut self.symbols[owner].declaration;
        let view = decl.view.as_mut()?;
        view.grouping = keys;
        let base = view.bases[0];
        let target = self.symbols[base].declaration.type_reference()?.clone();
        let element = self.types.len();
        self.types.push(TypeInfo::Structure(StructureType {
            target: Some(target),
            restrictions: Vec::new(),
        }));
        let ty = self.types.len();
        self.types.push(TypeInfo::Collection(CollectionType {
            kind: CollectionKind::Bag,
            cardinality: Cardinality {
                lower: "0".into(),
                upper: None,
            },
            element,
        }));
        let name = crate::syntax::Name {
            value: "AGGREGATES".into(),
            span: at.clone(),
        };
        let id = self.add(
            Kind::Attribute,
            &name,
            at,
            Some(owner),
            self.symbols[owner].model,
            None,
        )?;
        self.symbols[id].declaration.type_id = Some(ty);
        self.symbols[id].declaration.transient = true;
        Some(())
    }

    fn bind_view_base(
        &mut self,
        owner: usize,
        base: &RenamedViewable,
        nullable: bool,
    ) -> Option<usize> {
        let target = self.expression_name(
            owner,
            &base.target,
            &[Kind::Class, Kind::Structure, Kind::Association, Kind::View],
            "ILI-VIEW-BASE",
        )?;
        let BoundName::Declared(source) = target else {
            self.unsupported_expression(&base.span);
            return None;
        };
        self.require_view_dependency(owner, source, &base.target.span)?;
        let name = base
            .alias
            .as_ref()
            .unwrap_or_else(|| base.target.segments.last().unwrap());
        let reference = self.bound_reference(owner, &base.target, target)?;
        let ty = self.object_selection_type(
            false,
            ObjectTarget {
                target: Some(reference.clone()),
                restrictions: Vec::new(),
            },
        );
        let id = self.add(
            Kind::ViewBase,
            name,
            &base.span,
            Some(owner),
            self.symbols[owner].model,
            None,
        )?;
        let decl = &mut self.symbols[id].declaration;
        decl.reference = Some(reference);
        decl.type_id = Some(ty);
        decl.nullable_view_base = nullable;
        Some(id)
    }

    fn require_view_dependency(&mut self, owner: usize, source: usize, at: &Span) -> Option<()> {
        if let (Some(topic), Some(target)) =
            (self.containing_topic(owner), self.containing_topic(source))
            && !self.specializes_symbol(topic, target, at)?
            && !self.topic_has_dependency(topic, target, at)?
        {
            self.diagnostics.push(
                "ILI-VIEW-TOPIC-DEPENDENCY",
                "A view using a basis from another topic requires DEPENDS ON that topic",
                Some(at.clone()),
            );
            return None;
        }
        Some(())
    }

    fn bind_view_members(
        &mut self,
        id: usize,
        documents: &BTreeMap<u32, &[Expression]>,
    ) -> Option<()> {
        if self.diagnostics.rejected {
            return None;
        }
        let Some(SyntaxDecl::View(view)) = self.symbols[id].ast else {
            unreachable!("view order contains only views");
        };
        let statements = documents[&view.span.sort_key().0];
        let union = self.symbols[id].declaration.view()?.formation == ViewFormation::Union;
        let mut attributes = Vec::new();
        if let Some(parent) = self.symbols[id].base_scope {
            let inherited = self.symbols[parent]
                .declaration
                .effective_attributes
                .as_deref()
                .unwrap_or_default();
            let work = inherited
                .iter()
                .map(|&member| self.symbols[member].declaration.name.len() + 1)
                .sum();
            if !self.step(work, &view.name.span) {
                return None;
            }
            attributes.extend_from_slice(
                self.symbols[parent]
                    .declaration
                    .effective_attributes
                    .as_deref()
                    .unwrap_or_default(),
            );
        }
        let inherited: BTreeMap<_, _> = attributes
            .iter()
            .enumerate()
            .map(|(position, &member)| (self.symbols[member].declaration.name.clone(), position))
            .collect();
        for attribute in &view.attributes {
            let (member, previous) = match attribute {
                ViewAttribute::Typed(item) => {
                    let SyntaxDecl::Attribute {
                        name,
                        properties,
                        derivations,
                        ..
                    } = item.as_ref()
                    else {
                        unreachable!("typed view member has attribute syntax");
                    };
                    if !union && derivations.len() > 1 {
                        self.unsupported_expression(item.span());
                        return None;
                    }
                    let member = self.path_attribute(id, name)?;
                    let position = inherited.get(&name.value).copied();
                    self.bind_view_attribute_base(
                        id,
                        member,
                        name,
                        properties,
                        position.map(|position| attributes[position]),
                    )?;
                    self.bind_attribute(member)?;
                    (member, position)
                }
                ViewAttribute::Derived {
                    name,
                    value,
                    properties,
                    span,
                } => {
                    let member = self.path_attribute(id, name)?;
                    let position = inherited.get(&name.value).copied();
                    self.bind_view_attribute_base(
                        id,
                        member,
                        name,
                        properties,
                        position.map(|position| attributes[position]),
                    )?;
                    let expression =
                        self.bind_expression(id, statements, *value, ExpressionContext::Value)?;
                    let (ty, mandatory) = if position.is_some() {
                        self.bind_attribute(member)?;
                        let decl = &self.symbols[member].declaration;
                        let ty = decl.type_id?;
                        let mandatory = decl.mandatory() == Some(true);
                        if !self.value_fits(
                            id,
                            expression,
                            ty,
                            mandatory,
                            super::functions::AssignmentContext::Attribute,
                        )? {
                            self.diagnostics.push(
                                "ILI-ATTRIBUTE-DERIVATION-TYPE",
                                "The derived value cannot be assigned to the inherited attribute type",
                                Some(self.expressions[expression.0 as usize].span.clone()),
                            );
                            return None;
                        }
                        (ty, mandatory)
                    } else {
                        (self.infer_view_attribute_type(expression, span)?, false)
                    };
                    let decl = &mut self.symbols[member].declaration;
                    decl.type_id = Some(ty);
                    decl.effective_mandatory = Some(mandatory);
                    decl.derivations = Some(Box::new([expression]));
                    (member, position)
                }
                ViewAttribute::AllOf(all) => {
                    if union {
                        self.diagnostics.push(
                            "ILI-VIEW-ALL-OF",
                            "ALL OF cannot select attributes for a UNION",
                            Some(all.span.clone()),
                        );
                        return None;
                    }
                    let Some(base) = self
                        .visible_name(
                            id,
                            &all.value.value,
                            Some(&[Kind::ViewBase]),
                            &all.value.span,
                        )
                        .ok()?
                    else {
                        self.diagnostics.push(
                            "ILI-VIEW-ALL-OF",
                            "ALL OF requires the local name of a view basis",
                            Some(all.value.span.clone()),
                        );
                        return None;
                    };
                    let decl = &self.symbols[base].declaration;
                    let source = self.names[decl.type_reference()?.target_name()];
                    let source = &self.symbols[source].declaration;
                    let with_roles =
                        self.models[self.symbols[id].model].language() == LanguageVersion::V24;
                    let roles = source
                        .association()
                        .filter(|_| with_roles)
                        .map_or(&[][..], |a| &a.roles);
                    let base_members = source.effective_attributes.as_deref().unwrap_or_default();
                    let amount = roles.len() + base_members.len();
                    if !self.step(amount, &all.span) {
                        return None;
                    }
                    let source = self.names[self.symbols[base]
                        .declaration
                        .type_reference()?
                        .target_name()];
                    let decl = &self.symbols[source].declaration;
                    let roles = decl
                        .association()
                        .filter(|_| with_roles)
                        .map_or(&[][..], |a| &a.roles);
                    let copied: Vec<_> = roles
                        .iter()
                        .chain(decl.effective_attributes.as_deref().unwrap_or_default())
                        .copied()
                        .collect();
                    for member in copied {
                        attributes
                            .push(self.copy_view_member(id, base, source, member, &all.value)?);
                    }
                    continue;
                }
            };
            if let Some(position) = previous {
                attributes[position] = member;
            } else {
                attributes.push(member);
            }
        }
        if !self.symbols[id].declaration.is_abstract()
            && let Some(member) = attributes
                .iter()
                .find(|&&member| self.symbols[member].declaration.is_abstract())
        {
            self.diagnostics.push(
                "ILI-VIEW-ABSTRACT-ATTRIBUTE",
                "A concrete view cannot have an abstract attribute",
                Some(self.symbols[*member].name_span.clone()),
            );
            return None;
        }
        let mut selections = Vec::new();
        for selection in &view.selections {
            let expr = self.bind_expression(
                id,
                statements,
                selection.expression,
                ExpressionContext::Value,
            )?;
            if self.expressions[expr.0 as usize].value_type != ExpressionType::Boolean {
                self.diagnostics.push(
                    "ILI-VIEW-SELECTION",
                    "A WHERE selection must be Boolean",
                    Some(self.expressions[expr.0 as usize].span.clone()),
                );
                return None;
            }
            selections.push(expr);
        }
        let decl = &mut self.symbols[id].declaration;
        decl.effective_attributes = Some(attributes.into_boxed_slice());
        decl.view.as_mut()?.selections.extend(selections);
        self.validate_subdivisions(&[id])
    }

    fn infer_view_attribute_type(&mut self, expression: ExpressionId, at: &Span) -> Option<usize> {
        let actual = underlying_expression(&self.expressions, expression);
        if let Some(ty) = actual.type_id {
            return Some(ty);
        }
        let ty = match &actual.kind {
            BoundExpressionKind::Boolean { .. } => TypeInfo::Boolean,
            BoundExpressionKind::Text {
                canonical_length, ..
            } if *canonical_length > 0 => TypeInfo::Text {
                max_length: canonical_length.to_string(),
            },
            BoundExpressionKind::Number {
                value: UnitConstant::Decimal(value),
                unit,
            } => TypeInfo::Numeric(NumericRange {
                bounds: Some(NumericBounds {
                    lower: value.clone(),
                    upper: value.clone(),
                }),
                unit: unit.clone(),
                reference_system: None,
                circular: false,
                direction: None,
            }),
            _ => {
                self.unsupported_expression(at);
                return None;
            }
        };
        let id = self.types.len();
        self.types.push(ty);
        let mut expression = expression;
        loop {
            let actual = &mut self.expressions[expression.0 as usize];
            actual.type_id = Some(id);
            if let BoundExpressionKind::Group { expression: child } = actual.kind {
                expression = child;
            } else {
                return Some(id);
            }
        }
    }

    fn bind_view_attribute_base(
        &mut self,
        owner: usize,
        member: usize,
        name: &crate::syntax::Name,
        properties: &[crate::syntax::Located<Property>],
        inherited: Option<usize>,
    ) -> Option<()> {
        let location = &name.span;
        if let Some(parent) = self.symbols[owner].base_scope
            && self
                .visible_name(parent, &name.value, Some(&[Kind::ViewBase]), location)
                .ok()?
                .is_some()
        {
            self.diagnostics.push(
                "ILI-VIEW-ALIAS-CONFLICT",
                "An attribute cannot hide an inherited view basis",
                Some(location.clone()),
            );
            return None;
        }
        let extended = properties.iter().any(|p| p.value == Property::Extended);
        let message = match (inherited, extended) {
            (Some(base), true) if !self.symbols[base].declaration.is_final() => {
                self.symbols[member].base_scope = Some(base);
                self.symbols[member].declaration.base_reference =
                    Some(self.reference_to_symbol(owner, base, name)?);
                return Some(());
            }
            (Some(_), true) => "A FINAL attribute cannot be extended",
            (Some(_), false) => "Overriding an inherited attribute requires EXTENDED",
            (None, true) => "EXTENDED requires an inherited attribute",
            (None, false) => return Some(()),
        };
        self.diagnostics
            .push("ILI-ATTRIBUTE-OVERRIDE", message, Some(location.clone()));
        None
    }

    fn copy_view_member(
        &mut self,
        owner: usize,
        base: usize,
        source: usize,
        member: usize,
        name: &crate::syntax::Name,
    ) -> Option<usize> {
        let decl = &self.symbols[member].declaration;
        let is_role = decl.kind == Kind::Role;
        if decl.is_abstract() && !self.symbols[owner].declaration.is_abstract() {
            self.diagnostics.push(
                "ILI-VIEW-ABSTRACT-ATTRIBUTE",
                "A concrete view cannot select an abstract basis member",
                Some(name.span.clone()),
            );
            return None;
        }
        let member_name = crate::syntax::Name {
            value: decl.name.clone(),
            span: name.span.clone(),
        };
        let source_ref = self.reference_to_symbol(owner, member, &member_name)?;
        let basis_ref = self.reference_to_symbol(owner, base, name)?;
        let ty = if is_role {
            self.role_value_type(owner, source, member, &name.span)?
        } else {
            self.symbols[member].declaration.type_id?
        };
        let steps = vec![basis_ref, source_ref.clone()]
            .into_iter()
            .map(|reference| BoundPathStep {
                reference,
                selection: None,
                selection_span: None,
            })
            .collect();
        let expression = self.record_expression(
            BoundExpressionKind::Path { steps },
            scalar_type(&self.types[ty]),
            Some(ty),
            &name.span,
        );
        let copy_id = self.add(
            Kind::Attribute,
            &member_name,
            &name.span,
            Some(owner),
            self.symbols[owner].model,
            None,
        )?;
        let inherited = if let Some(parent) = self.symbols[owner].base_scope {
            self.visible_name(
                parent,
                &member_name.value,
                Some(&[Kind::Attribute]),
                &name.span,
            )
            .ok()?
        } else {
            None
        };
        self.bind_view_attribute_base(owner, copy_id, &member_name, &[], inherited)?;
        let optional = self.symbols[base].declaration.is_nullable_view_base();
        let [copy, original] = self
            .symbols
            .get_disjoint_mut([copy_id, member])
            .expect("member copy has its own ID");
        copy.declaration.type_id = Some(ty);
        copy.declaration.effective_mandatory =
            Some(!is_role && !optional && original.declaration.mandatory() == Some(true));
        copy.declaration.derived_from = Some(source_ref);
        copy.declaration.final_declaration = original.declaration.is_final();
        copy.declaration.abstract_declaration = original.declaration.is_abstract();
        copy.declaration.transient = !is_role && original.declaration.is_transient();
        copy.declaration.subdivision = original.declaration.subdivision.clone();
        copy.declaration.derivations = Some(Box::new([expression]));
        Some(copy_id)
    }

    pub(super) fn validate_view_derivations(&mut self) -> Option<()> {
        let mut dependencies = BTreeMap::new();
        for id in 0..self.symbols.len() {
            let decl = &self.symbols[id].declaration;
            if decl.kind != Kind::Attribute
                || !decl
                    .parent
                    .is_some_and(|owner| self.symbols[owner].declaration.kind == Kind::View)
            {
                continue;
            }
            dependencies.insert(id, self.view_attribute_dependencies(id)?);
        }
        if dependencies.is_empty() {
            return Some(());
        }
        let mut state = vec![0; self.symbols.len()];
        let mut nullable = vec![false; self.symbols.len()];
        let mut union_bases = BTreeMap::<usize, BTreeSet<usize>>::new();
        let mut stack = Vec::new();
        for &id in dependencies.keys() {
            if state[id] != 0 {
                continue;
            }
            stack.push((id, 0));
            state[id] = 1;
            while let Some((member, next)) = stack.last_mut() {
                let dep = &dependencies[member];
                let Some((target, at)) = dep.attributes.get(*next) else {
                    state[*member] = 2;
                    nullable[*member] =
                        dep.nullable || dep.attributes.iter().any(|(target, _)| nullable[*target]);
                    let decl = &self.symbols[*member].declaration;
                    let at = decl
                        .derivations()
                        .first()
                        .map_or_else(
                            || decl.source_span(),
                            |id| &self.expressions[id.0 as usize].span,
                        )
                        .clone();
                    if nullable[*member] && decl.mandatory() == Some(true) {
                        self.diagnostics.push(
                            "ILI-VIEW-NULLABLE-ATTRIBUTE",
                            "A mandatory view attribute cannot use an optional basis, including through another attribute",
                            Some(at),
                        );
                        return None;
                    }
                    let owner = decl.parent?;
                    if let Some(view) = self.symbols[owner].declaration.view()
                        && view.formation == ViewFormation::Union
                    {
                        if !self.step(view.bases.len() + dep.bases.len(), &at) {
                            return None;
                        }
                        let mut covered = dep.bases.clone();
                        for (target, _) in &dep.attributes {
                            if let Some(inherited) = union_bases.get(target) {
                                if !self.step(inherited.len(), &at) {
                                    return None;
                                }
                                covered.extend(inherited);
                            }
                        }
                        let view = self.symbols[owner].declaration.view()?;
                        if self.symbols[*member].declaration.mandatory() == Some(true)
                            && view.bases.iter().any(|basis| {
                                self.symbols[*basis].declaration.base_reference().is_none()
                                    && !covered.contains(basis)
                            })
                        {
                            self.diagnostics.push(
                                "ILI-VIEW-UNION-ATTRIBUTE",
                                "A mandatory UNION attribute needs a derivation for every basis",
                                Some(at),
                            );
                            return None;
                        }
                        union_bases.insert(*member, covered);
                    }
                    stack.pop();
                    continue;
                };
                *next += 1;
                if !self.step(1, at) {
                    return None;
                }
                match state[*target] {
                    0 => {
                        state[*target] = 1;
                        stack.push((*target, 0));
                    }
                    1 => {
                        self.diagnostics.push(
                            "ILI-VIEW-ATTRIBUTE-CYCLE",
                            "This reference closes a cycle of view attribute derivations",
                            Some(at.clone()),
                        );
                        return None;
                    }
                    _ => {}
                }
            }
        }
        Some(())
    }

    fn view_attribute_dependencies(&mut self, member: usize) -> Option<ViewAttributeDependencies> {
        let decl = &self.symbols[member].declaration;
        let union = decl
            .parent
            .and_then(|owner| self.symbols[owner].declaration.view())
            .is_some_and(|view| view.formation == ViewFormation::Union);
        let mut pending = decl.derivations().to_vec();
        let mut result = ViewAttributeDependencies::default();
        if let Some(parent) = self.symbols[member].base_scope {
            result
                .attributes
                .push((parent, decl.base_reference()?.source_span().clone()));
        }
        while let Some(id) = pending.pop() {
            let at = self.expressions[id.0 as usize].span.clone();
            if !self.step(1, &at) {
                return None;
            }
            match &self.expressions[id.0 as usize].kind {
                BoundExpressionKind::Path { steps } => {
                    for step in steps {
                        let id = self.names[step.reference.target_name()];
                        let target = &self.symbols[id].declaration;
                        result.nullable |= target.is_nullable_view_base();
                        if union && target.kind == Kind::ViewBase {
                            result.bases.insert(id);
                        }
                        if target.kind == Kind::Attribute
                            && target.parent.is_some_and(|owner| {
                                self.symbols[owner].declaration.kind == Kind::View
                            })
                        {
                            result
                                .attributes
                                .push((id, step.reference.source_span().clone()));
                        }
                    }
                }
                BoundExpressionKind::Call { arguments, .. } => {
                    pending.extend(arguments.iter().copied())
                }
                BoundExpressionKind::Binary { left, right, .. } => pending.extend([*left, *right]),
                BoundExpressionKind::Unary { operand, .. } => pending.push(*operand),
                BoundExpressionKind::Group { expression }
                | BoundExpressionKind::Defined { expression } => pending.push(*expression),
                _ => {}
            }
        }
        Some(result)
    }
}
