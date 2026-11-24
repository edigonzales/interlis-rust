use super::{
    Analyzer, BoundName, Kind, SyntaxDecl, TypeInfo,
    expressions::{ExpressionContext, scalar_type, underlying_expression},
};
use crate::{
    BoundExpressionKind, BoundPathStep, ExpressionId, ExpressionType, LanguageVersion,
    ObjectTarget, Span, ViewFormation, ViewInfo,
    syntax::{Expression, Property, RenamedViewable, ViewAttribute, ViewOrigin},
};
use std::collections::BTreeMap;

impl Analyzer<'_, '_> {
    pub(super) fn bind_views(
        &mut self,
        documents: &BTreeMap<u32, &[Expression]>,
    ) -> Option<Vec<usize>> {
        let mut views = Vec::new();
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::View(view)) = self.symbols[id].ast else {
                continue;
            };
            let Some(ViewOrigin::Formation(formation)) = &view.origin else {
                self.diagnostics.push(
                    "ILI-VIEW-FORMATION",
                    "A view requires a formation or an inherited view",
                    Some(view.name.span.clone()),
                );
                return None;
            };
            let mut bases = Vec::new();
            let kind = match &formation.value {
                crate::syntax::ViewFormation::Projection(base) => {
                    bases.push(self.bind_view_base(id, base, false)?);
                    ViewFormation::Projection
                }
                crate::syntax::ViewFormation::Join { first, others } => {
                    bases.push(self.bind_view_base(id, first, false)?);
                    for base in others {
                        bases.push(self.bind_view_base(id, &base.base, base.or_null.is_some())?);
                    }
                    ViewFormation::Join
                }
                _ => unreachable!("unsupported view formations are gated before binding"),
            };
            self.symbols[id].declaration.view = Some(Box::new(ViewInfo {
                formation: kind,
                bases,
                selections: Vec::new(),
            }));
            views.push(id);
        }
        // Every source view must have its members before a view can project them.
        let mut state = vec![0; self.symbols.len()];
        let mut stack = Vec::new();
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
                let info = self.symbols[*view].declaration.view()?;
                let Some(&base) = info.bases.get(*next) else {
                    self.bind_view_members(*view, documents)?;
                    state[*view] = 2;
                    stack.pop();
                    continue;
                };
                *next += 1;
                let base = self.symbols[base].declaration.type_reference()?;
                let source = self.names[base.target_name()];
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
        if let (Some(topic), Some(target)) =
            (self.containing_topic(owner), self.containing_topic(source))
            && !self.specializes_symbol(topic, target, &base.span)?
            && !self.topic_has_dependency(topic, target, &base.span)?
        {
            self.diagnostics.push(
                "ILI-VIEW-TOPIC-DEPENDENCY",
                "A view using a basis from another topic requires DEPENDS ON that topic",
                Some(base.target.span.clone()),
            );
            return None;
        }
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
        let mut attributes = Vec::new();
        for attribute in &view.attributes {
            let member = match attribute {
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
                    if derivations.len() > 1 {
                        self.unsupported_expression(item.span());
                        return None;
                    }
                    self.check_view_member_properties(properties)?;
                    let member = self.path_attribute(id, name)?;
                    self.bind_attribute(member)?;
                    member
                }
                ViewAttribute::Derived {
                    name,
                    value,
                    properties,
                    span,
                } => {
                    self.check_view_member_properties(properties)?;
                    let member = self.path_attribute(id, name)?;
                    let expression =
                        self.bind_expression(id, statements, *value, ExpressionContext::Value)?;
                    let actual = underlying_expression(&self.expressions, expression);
                    let ty = match (actual.type_id, &actual.kind) {
                        (Some(ty), _) => ty,
                        (None, BoundExpressionKind::Boolean { .. }) => {
                            let ty = self.types.len();
                            self.types.push(TypeInfo::Boolean);
                            ty
                        }
                        _ => {
                            self.unsupported_expression(span);
                            return None;
                        }
                    };
                    self.expressions[expression.0 as usize].type_id = Some(ty);
                    let decl = &mut self.symbols[member].declaration;
                    decl.type_id = Some(ty);
                    decl.effective_mandatory = Some(false);
                    decl.derivations = Some(Box::new([expression]));
                    member
                }
                ViewAttribute::AllOf(all) => {
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
            if self.symbols[member].declaration.is_abstract()
                && !self.symbols[id].declaration.is_abstract()
            {
                self.diagnostics.push(
                    "ILI-VIEW-ABSTRACT-ATTRIBUTE",
                    "A concrete view cannot declare an abstract attribute",
                    Some(self.symbols[member].name_span.clone()),
                );
                return None;
            }
            attributes.push(member);
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
        decl.view.as_mut()?.selections = selections;
        self.validate_subdivisions(&[id])
    }

    fn check_view_member_properties(
        &mut self,
        properties: &[crate::syntax::Located<Property>],
    ) -> Option<()> {
        if let Some(property) = properties.iter().find(|p| p.value == Property::Extended) {
            self.diagnostics.push(
                "ILI-ATTRIBUTE-OVERRIDE",
                "EXTENDED requires a view with an inherited attribute",
                Some(property.span.clone()),
            );
            return None;
        }
        Some(())
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

    pub(super) fn view_uses_nullable_base(&mut self, expression: ExpressionId) -> Option<bool> {
        let mut pending = vec![expression];
        while let Some(id) = pending.pop() {
            let at: Span = self.expressions[id.0 as usize].span.clone();
            if !self.step(1, &at) {
                return None;
            }
            match &self.expressions[id.0 as usize].kind {
                BoundExpressionKind::Path { steps } => {
                    if steps.iter().any(|step| {
                        self.symbols[self.names[step.reference.target_name()]]
                            .declaration
                            .is_nullable_view_base()
                    }) {
                        return Some(true);
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
        Some(false)
    }
}
