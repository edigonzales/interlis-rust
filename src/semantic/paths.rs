use super::{
    Analyzer, BoundName, Kind, LookupError, Name, QualifiedName, TypeInfo, TypeSyntax,
    associations::AssociationAccess,
    collections::{compare_count, structure_targets},
    domains::scalar_extension,
    expressions::{scalar_type, underlying_expression},
    functions::text_details,
    object_references::object_targets,
};
use crate::{
    AssociationPathStep, AttributePathOwner, AttributePathType, BoundExpressionKind, BoundPathStep,
    CollectionKind, CoordinateKind, ExpressionId, ExpressionType, LanguageVersion, ObjectSelection,
    PathSelection, Span, TypeReference,
    model::AttributeRestriction,
    syntax::{AttributePathOf, ObjectPath, PathElement, PathSelector, SpecialPath, TypeUseSyntax},
};
use std::cmp::Ordering;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum PathContext {
    Object,
    RoleSet,
    AreaArgument,
    UniquePrefix(LanguageVersion),
}

impl Analyzer<'_, '_> {
    pub(super) fn attribute_path_type(
        &mut self,
        owner: usize,
        of: Option<&AttributePathOf>,
        restrictions: &[TypeUseSyntax],
    ) -> Option<AttributePathType> {
        if let Some(AttributePathOf::Path(path)) = of
            && self.symbols[owner].declaration.kind != Kind::Attribute
        {
            self.diagnostics.push(
                "ILI-ATTRIBUTE-PATH-OWNER",
                "ATTRIBUTE OF requires the context of a class or structure attribute",
                Some(path.span.clone()),
            );
            return None;
        }
        let formal = if let Some(AttributePathOf::Argument(name)) = of {
            let declaration = &self.symbols[owner].declaration;
            let function = if declaration.kind == Kind::Function {
                Some(owner)
            } else if declaration.kind == Kind::Parameter {
                declaration
                    .parent
                    .filter(|&parent| self.symbols[parent].declaration.kind == Kind::Function)
            } else {
                None
            };
            let Some(function) = function else {
                return self.path_error(
                    "ILI-ATTRIBUTE-PATH-ARGUMENT",
                    "ATTRIBUTE OF @ requires a function signature",
                    &name.span,
                );
            };
            let Some(target) = self
                .visible_name(function, &name.value, Some(&[Kind::Parameter]), &name.span)
                .ok()?
            else {
                return self.path_error(
                    "ILI-ATTRIBUTE-PATH-ARGUMENT",
                    "The named formal argument is absent from this function",
                    &name.span,
                );
            };
            Some(AttributePathOwner::Argument(
                self.reference_to_symbol(owner, target, name)?,
            ))
        } else {
            None
        };
        let reference = self.symbols[owner].declaration.reference.take();
        let restrictions = restrictions
            .iter()
            .map(|restriction| {
                self.concrete_type(owner, restriction.ty.as_ref()?)
                    .map(|effective| AttributeRestriction {
                        ty: effective.ty,
                        mandatory: restriction.mandatory || effective.mandatory,
                    })
            })
            .collect::<Option<Vec<_>>>();
        self.symbols[owner].declaration.reference = reference;
        Some(AttributePathType {
            owner: formal,
            restrictions: restrictions?,
        })
    }

    pub(super) fn bind_attribute_path_owners(
        &mut self,
        symbols: std::ops::Range<usize>,
    ) -> Option<()> {
        for id in symbols {
            let symbol = &self.symbols[id];
            if symbol.declaration.kind != Kind::Attribute {
                continue;
            }
            let (Some(syntax), Some(ty), Some(owner)) = (
                symbol.syntax,
                symbol.declaration.type_id,
                symbol.declaration.parent,
            ) else {
                continue;
            };
            let mut pending = vec![(syntax, ty)];
            while let Some((syntax, ty)) = pending.pop() {
                if !self.step(1, syntax.span()) {
                    return None;
                }
                match (syntax, &self.types[ty]) {
                    (TypeSyntax::Collection { element, .. }, TypeInfo::Collection(collection)) => {
                        pending.push((element.as_ref(), collection.element));
                    }
                    (
                        TypeSyntax::AttributePathValue {
                            of, restrictions, ..
                        },
                        TypeInfo::AttributePath(attribute),
                    ) => {
                        pending.extend(
                            restrictions.iter().zip(&attribute.restrictions).filter_map(
                                |(syntax, restriction)| Some((syntax.ty.as_ref()?, restriction.ty)),
                            ),
                        );
                        let Some(AttributePathOf::Path(path)) = of else {
                            continue;
                        };
                        match self.expression_path(owner, path) {
                            Some((kind, ExpressionType::Class, type_id)) => {
                                let expression = self.record_expression(
                                    kind,
                                    ExpressionType::Class,
                                    type_id,
                                    &path.span,
                                );
                                let TypeInfo::AttributePath(attribute) = &mut self.types[ty] else {
                                    unreachable!()
                                };
                                attribute.owner =
                                    Some(AttributePathOwner::ClassAttribute(expression));
                            }
                            Some(_) => self.diagnostics.push(
                                "ILI-ATTRIBUTE-PATH-OWNER-TYPE",
                                "The ATTRIBUTE OF path must select a class-valued attribute",
                                Some(path.span.clone()),
                            ),
                            None if self.diagnostics.limited => return None,
                            None => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        for id in 0..self.symbols.len() {
            let symbol = &self.symbols[id];
            if symbol.declaration.kind != Kind::Attribute {
                continue;
            }
            let Some(own) = symbol
                .declaration
                .type_id
                .and_then(|ty| self.attribute_path_leaf(ty))
            else {
                continue;
            };
            let Some(base) = symbol
                .base_scope
                .and_then(|base| self.symbols[base].declaration.type_id)
                .and_then(|ty| self.attribute_path_leaf(ty))
            else {
                continue;
            };
            let span = symbol.declaration.span.clone();
            if !self.attribute_path_specializes(id, own, base, &span, None)? {
                self.report_type_override(
                    id,
                    "ILI-ATTRIBUTE-PATH-EXTENSION",
                    "The ATTRIBUTE extension must preserve its inherited class-valued path",
                    &span,
                );
            }
        }
        Some(())
    }

    fn attribute_path_leaf(&self, mut ty: usize) -> Option<usize> {
        while let TypeInfo::Collection(collection) = &self.types[ty] {
            ty = collection.element;
        }
        matches!(self.types[ty], TypeInfo::AttributePath(_)).then_some(ty)
    }

    pub(super) fn attribute_path_literal(
        &mut self,
        owner: usize,
        viewable: Option<&QualifiedName>,
        attribute: &Name,
        contextual_scope: Option<usize>,
    ) -> Option<(BoundExpressionKind, ExpressionType, Option<usize>)> {
        let (scope, selected) = if let Some(name) = viewable {
            let target = self.expression_name(
                owner,
                name,
                &[Kind::Class, Kind::Structure, Kind::Association, Kind::View],
                "ILI-ATTRIBUTE-PATH-VIEWABLE",
            )?;
            let BoundName::Declared(scope) = target else {
                self.unsupported_expression(&name.span);
                return None;
            };
            (scope, self.bound_reference(owner, name, target)?)
        } else {
            let scope = contextual_scope.unwrap_or(owner);
            let name = Name {
                value: self.symbols[scope].declaration.name.clone(),
                span: self.symbols[scope].name_span.clone(),
            };
            (scope, self.reference_to_symbol(owner, scope, &name)?)
        };
        let member = self.path_attribute(scope, attribute)?;
        let member_declaration = &self.symbols[member].declaration;
        let restriction = AttributeRestriction {
            ty: member_declaration.type_id?,
            mandatory: member_declaration.mandatory() == Some(true),
        };
        let reference = self.reference_to_symbol(owner, member, attribute)?;
        let id = self.types.len();
        self.types.push(TypeInfo::AttributePath(AttributePathType {
            owner: Some(AttributePathOwner::Viewable(selected)),
            restrictions: vec![restriction],
        }));
        Some((
            BoundExpressionKind::AttributePath { reference },
            ExpressionType::AttributePath,
            Some(id),
        ))
    }

    pub(super) fn attribute_path_specializes(
        &mut self,
        owner: usize,
        actual: usize,
        expected: usize,
        at: &Span,
        value: Option<ExpressionId>,
    ) -> Option<bool> {
        // The declared selector starts at the object containing the value's attribute.
        let prefix = value
            .and_then(|id| {
                let expression = underlying_expression(&self.expressions, id);
                let BoundExpressionKind::Path { steps } = &expression.kind else {
                    return None;
                };
                steps.split_last().map(|(_, prefix)| prefix)
            })
            .unwrap_or_default();
        if actual == expected && prefix.is_empty() {
            return Some(true);
        }
        let (TypeInfo::AttributePath(source), TypeInfo::AttributePath(target)) =
            (&self.types[actual], &self.types[expected])
        else {
            return Some(false);
        };
        let same_owner = match (&source.owner, &target.owner) {
            (_, None) => true,
            (
                Some(AttributePathOwner::Viewable(source)),
                Some(AttributePathOwner::Viewable(target)),
            ) => source.target_name() == target.target_name(),
            (
                Some(AttributePathOwner::Argument(source)),
                Some(AttributePathOwner::Argument(target)),
            ) => source.target_name() == target.target_name(),
            (
                Some(AttributePathOwner::ClassAttribute(source)),
                Some(AttributePathOwner::ClassAttribute(target)),
            ) if prefix.is_empty() => {
                match super::translations::same_bound_expression(
                    &self.types,
                    &self.expressions,
                    &self.names,
                    &self.temporal_languages,
                    &mut self.work,
                    *source,
                    *target,
                ) {
                    Ok(equal) => equal,
                    Err(()) => {
                        self.diagnostics
                            .limit("Semantic work budget exceeded", Some(at.clone()));
                        return None;
                    }
                }
            }
            (
                Some(AttributePathOwner::ClassAttribute(source)),
                Some(AttributePathOwner::ClassAttribute(target)),
            ) => {
                match (
                    &underlying_expression(&self.expressions, *source).kind,
                    &underlying_expression(&self.expressions, *target).kind,
                ) {
                    (
                        BoundExpressionKind::Path { steps: source },
                        BoundExpressionKind::Path { steps: target },
                    ) => {
                        if self
                            .work
                            .charge((prefix.len() + source.len() + target.len()) as u32)
                            .is_err()
                        {
                            self.diagnostics
                                .limit("Semantic work budget exceeded", Some(at.clone()));
                            return None;
                        }
                        prefix.len() + source.len() == target.len()
                            && prefix
                                .iter()
                                .chain(source)
                                .zip(target)
                                .all(|(source, target)| {
                                    source.reference.target_name() == target.reference.target_name()
                                        && equivalent_selector(
                                            source.selection.as_ref(),
                                            target.selection.as_ref(),
                                        )
                                })
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !same_owner {
            return Some(false);
        }
        if target.restrictions.is_empty() {
            return Some(true);
        }
        if source.restrictions.is_empty() {
            return Some(false);
        }
        if !self.step(source.restrictions.len() + target.restrictions.len(), at) {
            return None;
        }
        let (TypeInfo::AttributePath(source), TypeInfo::AttributePath(target)) =
            (&self.types[actual], &self.types[expected])
        else {
            unreachable!();
        };
        let sources = source.restrictions.clone();
        let targets = target.restrictions.clone();
        for &source in &sources {
            let mut fits = false;
            for &target in &targets {
                if (!target.mandatory || source.mandatory)
                    && self.attribute_type_match(owner, source.ty, target.ty, at, value)?
                {
                    fits = true;
                    break;
                }
            }
            if !fits {
                return Some(false);
            }
        }
        Some(true)
    }

    pub(super) fn attribute_type_match(
        &mut self,
        owner: usize,
        source: usize,
        target: usize,
        at: &Span,
        value: Option<ExpressionId>,
    ) -> Option<bool> {
        if !self.step(1, at) {
            return None;
        }
        if source == target
            && (value.is_none()
                || !matches!(
                    self.types[source],
                    TypeInfo::AttributePath(_) | TypeInfo::Collection(_)
                ))
        {
            return Some(true);
        }
        if self.is_enum_type(source) && self.is_enum_type(target) {
            return self.enum_specializes(source, target, at);
        }
        if let Some((source_multi, source_max, source_format)) = text_details(&self.types[source])
            && let Some((target_multi, target_max, target_format)) =
                text_details(&self.types[target])
        {
            let fits = (!source_multi || target_multi)
                && (target_format.is_none() || source_format == target_format)
                && target_max.is_none_or(|max| {
                    source_max.is_some_and(|given| compare_count(given, max) != Ordering::Greater)
                });
            return Some(fits);
        }
        match (&self.types[source], &self.types[target]) {
            (TypeInfo::Reference(_), TypeInfo::Reference(_)) => {
                return self.reference_specializes(source, target, at);
            }
            (TypeInfo::Structure(_), TypeInfo::Structure(_)) => {
                return self.structure_specializes(source, target, at);
            }
            (TypeInfo::ClassValue(_), TypeInfo::ClassValue(_)) => {
                return self.class_value_specializes(source, target, at, false);
            }
            (TypeInfo::Oid(a), TypeInfo::Oid(b)) => {
                return match super::oid::oid_value_fits(a, b, &mut self.work) {
                    Ok(fits) => Some(fits),
                    Err(error) => {
                        self.number_error(error, at);
                        None
                    }
                };
            }
            (
                TypeInfo::Temporal(_) | TypeInfo::TemporalRange(_),
                TypeInfo::Temporal(_) | TypeInfo::TemporalRange(_),
            ) => {
                return Some(self.preserves_temporal_layout(source, target));
            }
            (TypeInfo::Collection(source), TypeInfo::Collection(target)) => {
                let cardinality =
                    compare_count(&source.cardinality.lower, &target.cardinality.lower)
                        != Ordering::Less
                        && target.cardinality.upper.as_deref().is_none_or(|upper| {
                            source.cardinality.upper.as_deref().is_some_and(|value| {
                                compare_count(value, upper) != Ordering::Greater
                            })
                        });
                if !cardinality
                    || (source.kind != target.kind && target.kind != CollectionKind::Bag)
                {
                    return Some(false);
                }
                return self.attribute_type_match(owner, source.element, target.element, at, value);
            }
            (TypeInfo::AttributePath(_), TypeInfo::AttributePath(_)) => {
                return self.attribute_path_specializes(owner, source, target, at, value);
            }
            (TypeInfo::Geometry(_), TypeInfo::Geometry(_)) => {
                return self.geometry_value_fits(source, target, at);
            }
            _ => {}
        }
        let language = self.models[self.symbols[owner].model].language();
        let check = self
            .check_unit_specialization(source, target, false)
            .and_then(|reason| {
                if reason.is_some() {
                    Ok(reason)
                } else {
                    scalar_extension(
                        &self.types[source],
                        &self.types[target],
                        language,
                        self.limits,
                        &mut self.work,
                        false,
                    )
                }
            });
        match check {
            Ok(reason) => Some(reason.is_none()),
            Err(error) => {
                self.number_error(error, at);
                None
            }
        }
    }

    pub(super) fn expression_path(
        &mut self,
        owner: usize,
        path: &ObjectPath,
    ) -> Option<(BoundExpressionKind, ExpressionType, Option<usize>)> {
        self.expression_path_with_context(owner, path, PathContext::Object)
    }

    pub(super) fn expression_path_with_context(
        &mut self,
        owner: usize,
        path: &ObjectPath,
        context: PathContext,
    ) -> Option<(BoundExpressionKind, ExpressionType, Option<usize>)> {
        let mut scope = self.expression_object(owner, &path.span)?;
        let mut steps = Vec::with_capacity(path.elements.len());
        let elements = match path.elements.split_first() {
            Some((PathElement::Special(special), tail)) if special.value == SpecialPath::This => {
                if tail.is_empty() {
                    let type_id = match self.symbols[owner].declaration.kind {
                        Kind::Class | Kind::Association | Kind::View => {
                            let target =
                                self.current_object_target(scope, "THIS", &special.span)?;
                            Some(self.object_selection_type(false, target))
                        }
                        Kind::Structure => {
                            let target =
                                self.current_object_target(scope, "THIS", &special.span)?;
                            let id = self.types.len();
                            self.types.push(TypeInfo::Structure(crate::StructureType {
                                target: target.target,
                                restrictions: target.restrictions,
                            }));
                            Some(id)
                        }
                        _ => self.symbols[owner].declaration.type_id,
                    };
                    let value_type =
                        type_id.map_or(ExpressionType::Object, |ty| scalar_type(&self.types[ty]));
                    return Some((BoundExpressionKind::This, value_type, type_id));
                }
                if self.symbols[owner].declaration.kind == Kind::Domain {
                    self.unsupported_expression(&path.span);
                    return None;
                }
                tail
            }
            _ => &path.elements,
        };
        if !self.step(elements.len(), &path.span) {
            return None;
        }
        let topic = self.expression_topic(owner);
        let mut plural_origin = None;
        for (position, element) in elements.iter().enumerate() {
            if let PathElement::Special(special) = element
                && matches!(
                    special.value,
                    SpecialPath::Parent | SpecialPath::ThisArea | SpecialPath::ThatArea
                )
            {
                let area = special.value != SpecialPath::Parent;
                let Some(inspection) = self.symbols[owner]
                    .declaration
                    .view()
                    .and_then(|view| view.inspection.as_ref())
                    .filter(|inspection| {
                        inspection.area == area
                            && (scope == owner
                                || self.names[inspection.element.target_name()] == scope)
                    })
                else {
                    return self.path_error(
                        "ILI-INSPECTION-CONTEXT",
                        if area {
                            "THISAREA and THATAREA require the element context of an area inspection"
                        } else {
                            "PARENT requires the element context of an ordinary inspection"
                        },
                        &special.span,
                    );
                };
                scope = self.names[inspection.parent.target_name()];
                let parent_type = inspection.parent_type;
                let name = Name {
                    value: match special.value {
                        SpecialPath::Parent => "PARENT",
                        SpecialPath::ThisArea => "THISAREA",
                        SpecialPath::ThatArea => "THATAREA",
                        _ => unreachable!(),
                    }
                    .into(),
                    span: special.span.clone(),
                };
                let reference = self.reference_to_symbol(owner, scope, &name)?;
                if position + 1 == elements.len() {
                    let ty = if let Some(ty) = parent_type {
                        ty
                    } else if self.symbols[scope].declaration.kind == Kind::Structure {
                        let ty = self.types.len();
                        self.types.push(TypeInfo::Structure(crate::StructureType {
                            target: Some(reference.clone()),
                            restrictions: Vec::new(),
                        }));
                        ty
                    } else {
                        self.object_selection_type(
                            false,
                            crate::ObjectTarget {
                                target: Some(reference.clone()),
                                restrictions: Vec::new(),
                            },
                        )
                    };
                    steps.push(BoundPathStep {
                        reference,
                        selection: None,
                        selection_span: None,
                        association_access: None,
                    });
                    return Some((
                        BoundExpressionKind::Path { steps },
                        ExpressionType::Object,
                        Some(ty),
                    ));
                }
                steps.push(BoundPathStep {
                    reference,
                    selection: None,
                    selection_span: None,
                    association_access: None,
                });
                continue;
            }
            let aggregate;
            let (name, selector, association_object) = match element {
                PathElement::Name { name, selector, .. } => (name, selector.as_ref(), false),
                PathElement::AssociationAccess { name, .. } => (name, None, true),
                PathElement::Special(special) if special.value == SpecialPath::Aggregates => {
                    aggregate = Name {
                        value: "AGGREGATES".into(),
                        span: special.span.clone(),
                    };
                    (&aggregate, None, false)
                }
                _ => {
                    self.unsupported_expression(&path.span);
                    return None;
                }
            };
            let qualifier = match selector {
                Some(PathSelector::Association(name)) => Some(name),
                _ => None,
            };
            let direct = !association_object && qualifier.is_none();
            let value_selector = selector.filter(|_| qualifier.is_none());
            let mut member = if direct {
                self.visible_name(
                    scope,
                    &name.value,
                    Some(&[Kind::Attribute, Kind::ViewBase]),
                    &name.span,
                )
                .ok()?
            } else {
                None
            };
            if member.is_none()
                && position == 0
                && let Some(view) = self.symbols[scope].declaration.view()
                && view.formation == crate::ViewFormation::Projection
            {
                let base = view.bases[0];
                scope = self.names[self.symbols[base]
                    .declaration
                    .type_reference()?
                    .target_name()];
                steps.push(BoundPathStep {
                    reference: self.reference_to_symbol(owner, base, name)?,
                    selection: None,
                    selection_span: None,
                    association_access: None,
                });
            }
            if member.is_none() && direct && value_selector.is_none() {
                member = self
                    .visible_name(scope, &name.value, Some(&[Kind::Role]), &name.span)
                    .ok()?;
            }
            let (value, step) = if let Some(member) = member {
                let decl = &self.symbols[member].declaration;
                if self.symbols[owner].declaration.kind == Kind::View
                    && !self.symbols[owner].declaration.is_abstract()
                    && decl.is_abstract()
                {
                    return self.path_error(
                        "ILI-VIEW-ABSTRACT-ATTRIBUTE",
                        "A concrete view cannot access an abstract basis member",
                        &name.span,
                    );
                }
                let member_type = if decl.kind == Kind::Role {
                    self.role_value_type(owner, scope, member, &name.span)?
                } else if let Some(ty) = decl.type_id {
                    ty
                } else {
                    if self.symbols[scope].declaration.kind == Kind::View {
                        self.unsupported_expression(&name.span);
                    }
                    return None;
                };
                let (value, selection, selection_span) = if let Some(selector) = value_selector {
                    let (value, selection, origin) =
                        self.select_path_value(member_type, selector)?;
                    (value, Some(selection), Some(origin))
                } else {
                    (member_type, None, None)
                };
                (
                    value,
                    BoundPathStep {
                        reference: self.reference_to_symbol(owner, member, name)?,
                        selection,
                        selection_span,
                        association_access: None,
                    },
                )
            } else if value_selector.is_none() {
                let first = position == 0 && elements.len() == path.elements.len();
                let chosen = if let Some(qualifier) = qualifier {
                    let path = QualifiedName {
                        segments: vec![Name {
                            value: qualifier.value.clone(),
                            span: qualifier.span.clone(),
                        }],
                        span: qualifier.span.clone(),
                    };
                    let BoundName::Declared(id) = self.expression_name(
                        owner,
                        &path,
                        &[Kind::Association],
                        "ILI-PATH-ASSOCIATION-QUALIFIER",
                    )?
                    else {
                        return None;
                    };
                    Some(id)
                } else {
                    None
                };
                let mut access = self.association_path_access(
                    owner,
                    scope,
                    name,
                    first,
                    chosen.is_some(),
                    chosen,
                );
                if direct && matches!(access, Ok(None)) {
                    access = self.association_path_access(owner, scope, name, first, true, None);
                }
                let access = match access {
                    Ok(Some(access)) => access,
                    Err(LookupError::Limit) => return None,
                    Err(LookupError::Deferred) => {
                        return self.path_error(
                            "ILI-PATH-ROLE-ORDER",
                            "The association must be declared before this role access",
                            &name.span,
                        );
                    }
                    Err(LookupError::Ambiguous(candidates)) => {
                        self.diagnostics.push_related(
                            "ILI-PATH-ROLE-AMBIGUOUS",
                            "The role can be reached through more than one association access",
                            Some(name.span.clone()),
                            candidates
                                .into_iter()
                                .map(|candidate| crate::RelatedLocation {
                                    span: self.symbols[candidate].name_span.clone(),
                                    message: "Candidate role access".into(),
                                })
                                .collect(),
                        );
                        return None;
                    }
                    _ => {
                        return self.path_error(
                            "ILI-PATH-NAME",
                            "The attribute or role is absent from this object context",
                            &name.span,
                        );
                    }
                };
                self.bind_association_path_step(owner, name, qualifier, access, association_object)?
            } else {
                return self.path_error(
                    "ILI-PATH-NAME",
                    "The attribute is absent from this object context",
                    &name.span,
                );
            };
            steps.push(step);
            if position + 1 == elements.len() {
                let value = if let Some(origin) = plural_origin {
                    match &mut self.types[value] {
                        TypeInfo::ObjectSelection(selection) => {
                            selection.plural = true;
                            value
                        }
                        TypeInfo::Reference(reference) => {
                            let target = reference.target.clone();
                            self.object_selection_type(true, target)
                        }
                        _ => {
                            return self.path_error(
                                "ILI-PATH-MULTIPLICITY",
                                "A path through multiple related objects must select objects",
                                &origin,
                            );
                        }
                    }
                } else {
                    value
                };
                return Some((
                    BoundExpressionKind::Path { steps },
                    scalar_type(&self.types[value]),
                    Some(value),
                ));
            }
            let context = if context == PathContext::AreaArgument {
                context
            } else if matches!(&self.types[value], TypeInfo::ObjectSelection(selection) if selection.plural)
            {
                plural_origin.get_or_insert_with(|| name.span.clone());
                PathContext::RoleSet
            } else {
                PathContext::Object
            };
            scope = self.path_scope(value, topic, context, &name.span)?;
        }
        unreachable!("a parsed path contains at least one step")
    }

    pub(super) fn path_attribute(&mut self, scope: usize, name: &Name) -> Option<usize> {
        match self.visible_name(scope, &name.value, Some(&[Kind::Attribute]), &name.span) {
            Ok(Some(member)) => Some(member),
            Err(LookupError::Limit) => None,
            _ => self.path_error(
                "ILI-PATH-NAME",
                "The attribute is absent from this object context",
                &name.span,
            ),
        }
    }

    pub(super) fn expression_object(&mut self, owner: usize, at: &Span) -> Option<usize> {
        if let Some(topic) = self.constraint_topic {
            self.contextual_path_scope(topic, owner, at)
        } else {
            Some(owner)
        }
    }

    fn bind_association_path_step(
        &mut self,
        owner: usize,
        name: &Name,
        qualifier: Option<&Name>,
        access: AssociationAccess,
        association_object: bool,
    ) -> Option<(usize, BoundPathStep)> {
        let association_object = association_object || access.target.is_none();
        let cardinality = self.access_cardinality(access, association_object, &name.span)?;
        let association =
            self.reference_to_symbol(owner, access.association, qualifier.unwrap_or(name))?;
        let source_role = self.reference_to_symbol(owner, access.source, name)?;
        let target_role = if let Some(target) = access.target {
            Some(self.reference_to_symbol(owner, target, name)?)
        } else {
            None
        };
        let plural = cardinality
            .upper
            .as_deref()
            .is_none_or(|upper| compare_count(upper, "1") == Ordering::Greater);
        let type_id = if association_object {
            self.object_selection_type(
                plural,
                crate::ObjectTarget {
                    target: Some(association.clone()),
                    restrictions: Vec::new(),
                },
            )
        } else {
            let target = access.target?;
            let id = self.role_value_type(owner, access.association, target, &name.span)?;
            let TypeInfo::ObjectSelection(selection) = &mut self.types[id] else {
                unreachable!("role has an object selection type");
            };
            selection.plural = plural;
            id
        };
        Some((
            type_id,
            BoundPathStep {
                reference: target_role.as_ref().unwrap_or(&source_role).clone(),
                selection: None,
                selection_span: None,
                association_access: Some(Box::new(AssociationPathStep {
                    association,
                    source_role,
                    target_role,
                    association_object,
                    cardinality,
                    type_id,
                })),
            },
        ))
    }

    pub(super) fn role_value_type(
        &mut self,
        owner: usize,
        scope: usize,
        role: usize,
        at: &Span,
    ) -> Option<usize> {
        let association = self.symbols[role].declaration.parent?;
        if !self.association_precedes_path(association, owner, at) {
            return self.path_error(
                "ILI-PATH-ROLE-ORDER",
                "The association must be declared before this role access",
                at,
            );
        }
        let info = self.symbols[role].declaration.role()?;
        let endpoint = self.symbols[scope]
            .declaration
            .association()
            .is_some_and(|association| association.roles.contains(&role));
        let degree = self.symbols[association]
            .declaration
            .association()?
            .roles
            .len();
        let plural = !endpoint
            && (degree != 2
                || info
                    .cardinality
                    .upper
                    .as_deref()
                    .is_none_or(|upper| compare_count(upper, "1") == Ordering::Greater));
        let bytes = info
            .targets
            .iter()
            .flat_map(|target| target.target.iter().chain(&target.restrictions))
            .map(|target| 1 + target.declared_name().len() + target.target_name().len())
            .sum();
        if !self.step(bytes, at) {
            return None;
        }
        let id = self.types.len();
        let targets = self.symbols[role].declaration.role()?.targets.clone();
        self.types.push(TypeInfo::ObjectSelection(ObjectSelection {
            plural,
            targets,
        }));
        Some(id)
    }

    fn select_path_value(
        &mut self,
        ty: usize,
        selector: &PathSelector,
    ) -> Option<(usize, PathSelection, Span)> {
        let (at, number) = match selector {
            PathSelector::First(span) | PathSelector::Last(span) => (span, None),
            PathSelector::Index(index) => {
                (&index.span, Some(self.literal(index)?.info.coefficient))
            }
            PathSelector::Association(_) => unreachable!("role qualifiers are bound separately"),
        };
        match &self.types[ty] {
            TypeInfo::Coordinate(coordinate) if coordinate.kind == CoordinateKind::Single => {
                let Some(axis) = number
                    .as_deref()
                    .and_then(|number| number.parse::<u8>().ok())
                    .filter(|&axis| axis > 0 && usize::from(axis) <= coordinate.axes.len())
                else {
                    return self.path_error(
                        "ILI-PATH-AXIS",
                        "A coordinate selector must name an existing axis",
                        at,
                    );
                };
                let index = usize::from(axis - 1);
                let range = &coordinate.axes[index];
                let numbers = range.bounds.as_ref().map_or(0, |bounds| {
                    bounds.lower.original().len()
                        + bounds.lower.coefficient().len()
                        + bounds.upper.original().len()
                        + bounds.upper.coefficient().len()
                });
                let names = range
                    .unit
                    .iter()
                    .chain(
                        range
                            .reference_system
                            .iter()
                            .map(crate::NumericReferenceSystem::target),
                    )
                    .map(|reference| {
                        reference.declared_name().len() + reference.target_name().len()
                    })
                    .sum::<usize>();
                if !self.step(1 + numbers + names, at) {
                    return None;
                }
                let TypeInfo::Coordinate(coordinate) = &self.types[ty] else {
                    unreachable!()
                };
                let axis_type = self.types.len();
                self.types
                    .push(TypeInfo::Numeric(coordinate.axes[index].clone()));
                Some((axis_type, PathSelection::Axis(axis), at.clone()))
            }
            TypeInfo::Collection(collection) if collection.kind == CollectionKind::List => {
                let index = number.as_deref().unwrap_or("1");
                if index == "0"
                    || collection
                        .cardinality
                        .upper
                        .as_deref()
                        .is_some_and(|upper| compare_count(index, upper) == Ordering::Greater)
                {
                    return self.path_error(
                        "ILI-PATH-INDEX",
                        "A list index must be between one and the maximum cardinality",
                        at,
                    );
                }
                let selection = match selector {
                    PathSelector::First(_) => PathSelection::First,
                    PathSelector::Last(_) => PathSelection::Last,
                    PathSelector::Index(_) => {
                        PathSelection::Index(number.expect("index was parsed"))
                    }
                    PathSelector::Association(_) => unreachable!(),
                };
                Some((collection.element, selection, at.clone()))
            }
            _ => self.path_error(
                "ILI-PATH-SELECTOR",
                "A selector requires a coordinate or an ordered list",
                at,
            ),
        }
    }

    pub(super) fn path_scope(
        &mut self,
        mut ty: usize,
        topic: Option<usize>,
        context: PathContext,
        at: &Span,
    ) -> Option<usize> {
        while let TypeInfo::Collection(collection) = &self.types[ty] {
            if context == PathContext::Object
                && collection
                    .cardinality
                    .upper
                    .as_deref()
                    .is_none_or(|upper| compare_count(upper, "1") == Ordering::Greater)
            {
                return self.path_error(
                    "ILI-PATH-MULTIPLICITY",
                    "A path through a collection requires a single element",
                    at,
                );
            }
            ty = collection.element;
            if !self.step(1, at) {
                return None;
            }
        }
        let roots = match &self.types[ty] {
            TypeInfo::Structure(structure) => {
                self.path_targets(structure_targets(structure).iter())
            }
            TypeInfo::Geometry(geometry)
                if context == PathContext::UniquePrefix(LanguageVersion::V23)
                    && geometry.line_attributes.is_some() =>
            {
                self.path_targets(geometry.line_attributes.iter())
            }
            _ if matches!(context, PathContext::UniquePrefix(_)) => {
                return self.path_error(
                    "ILI-UNIQUE-PREFIX",
                    "A LOCAL key prefix must use structure attributes",
                    at,
                );
            }
            TypeInfo::Reference(reference) => {
                self.path_targets(object_targets(&reference.target).iter())
            }
            TypeInfo::ObjectSelection(selection) => {
                if selection.plural
                    && !matches!(context, PathContext::RoleSet | PathContext::AreaArgument)
                {
                    return self.path_error(
                        "ILI-PATH-MULTIPLICITY",
                        "A path through a role requires a single related object",
                        at,
                    );
                }
                if selection
                    .targets
                    .iter()
                    .any(|target| object_targets(target).is_empty())
                {
                    self.unsupported_expression(at);
                    return None;
                }
                self.path_targets(selection.targets.iter().flat_map(object_targets))
            }
            _ => {
                return self.path_error(
                    "ILI-PATH-TRAVERSAL",
                    "The preceding value cannot be traversed as an object",
                    at,
                );
            }
        };
        let Some(mut roots) = roots.filter(|roots| !roots.is_empty()) else {
            self.unsupported_expression(at);
            return None;
        };
        if !self.step(roots.len(), at) {
            return None;
        }
        if let Some(topic) = topic {
            for root in &mut roots {
                *root = self.contextual_path_scope(topic, *root, at)?;
            }
        }
        self.common_path_scope(&roots, at)
    }

    pub(super) fn common_path_scope(&mut self, roots: &[usize], at: &Span) -> Option<usize> {
        let Some(&first) = roots.first() else {
            self.unsupported_expression(at);
            return None;
        };
        let mut base = Some(first);
        while let Some(candidate) = base {
            if !self.step(1, at) {
                return None;
            }
            let mut common = true;
            for &other in &roots[1..] {
                if !self.specializes_symbol(other, candidate, at)? {
                    common = false;
                    break;
                }
            }
            if common {
                return Some(candidate);
            }
            base = self.symbols[candidate].base_scope;
        }
        self.path_error(
            "ILI-PATH-SCOPE",
            "The possible objects have no common declared scope for this path",
            at,
        )
    }

    fn path_targets<'a>(
        &self,
        targets: impl Iterator<Item = &'a TypeReference>,
    ) -> Option<Vec<usize>> {
        targets
            .map(|target| self.names.get(target.target_name()).copied())
            .collect()
    }

    pub(super) fn contextual_path_scope(
        &mut self,
        topic: usize,
        root: usize,
        at: &Span,
    ) -> Option<usize> {
        let Some(base_topic) = self.containing_topic(root) else {
            return Some(root);
        };
        if topic == base_topic || !self.specializes_symbol(topic, base_topic, at)? {
            return Some(root);
        }
        let declaration = &self.symbols[root].declaration;
        let name = declaration.name.clone();
        let kind = declaration.kind;
        if let Some(local) = self.visible_name(topic, &name, Some(&[kind]), at).ok()?
            && self.specializes_symbol(local, root, at)?
        {
            return Some(local);
        }
        Some(root)
    }

    fn path_error<T>(&mut self, code: &str, message: &str, at: &Span) -> Option<T> {
        self.diagnostics.push(code, message, Some(at.clone()));
        None
    }
}

pub(super) fn equivalent_selector(
    source: Option<&PathSelection>,
    target: Option<&PathSelection>,
) -> bool {
    source == target
        || match (source, target) {
            (Some(PathSelection::First), Some(PathSelection::Index(index)))
            | (Some(PathSelection::Index(index)), Some(PathSelection::First)) => index == "1",
            _ => false,
        }
}
