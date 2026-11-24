use super::{
    Analyzer, BoundName, Kind, TypeInfo, collections::compare_count, domains::scalar_extension,
    expressions::scalar_type,
};
use crate::{
    BoundConstraint, BoundExpressionKind, ConstraintRule, ExistenceTarget, ExpressionId,
    ExpressionType, NumericRange, OidType, Span, TypeReference,
    syntax::{ConstraintSyntax, ExistenceTarget as TargetSyntax, ObjectPath},
};
use std::cmp::Ordering;

impl Analyzer<'_, '_> {
    pub(super) fn bind_existence(
        &mut self,
        owner: usize,
        syntax: &ConstraintSyntax,
        path: &ObjectPath,
        targets: &[TargetSyntax],
    ) -> Option<BoundConstraint> {
        let (source, actual) = self.existence_path(owner, path)?;
        let topic = self.containing_topic(owner);
        let mut required = Vec::with_capacity(targets.len());
        for target in targets {
            let name = &target.viewable;
            let bound = self.expression_name(
                owner,
                name,
                &[Kind::Class, Kind::Association, Kind::View],
                "ILI-EXISTENCE-TARGET",
            )?;
            let BoundName::Declared(scope) = bound else {
                self.unsupported_expression(&name.span);
                return None;
            };
            if let (Some(topic), Some(other)) = (topic, self.containing_topic(scope))
                && !self.specializes_symbol(topic, other, &name.span)?
                && !self.topic_has_dependency(topic, other, &name.span)?
            {
                self.diagnostics.push(
                    "ILI-EXISTENCE-DEPENDS-ON",
                    "An EXISTENCE constraint targeting another topic requires DEPENDS ON",
                    Some(name.span.clone()),
                );
                return None;
            }
            let (path, expected) = self.existence_path(scope, &target.path)?;
            if !self.existence_compatible(owner, actual, expected, &target.path.span)? {
                self.diagnostics.push_related(
                    "ILI-EXISTENCE-TYPE",
                    "The required attribute must have a compatible value type",
                    Some(target.path.span.clone()),
                    vec![crate::RelatedLocation {
                        span: self.expressions[source.0 as usize].span.clone(),
                        message: "Source attribute".into(),
                    }],
                );
                return None;
            }
            required.push(ExistenceTarget {
                viewable: self.bound_reference(owner, name, bound)?,
                path,
            });
        }
        Some(BoundConstraint {
            name: syntax.name.as_ref().map(|name| name.value.clone()),
            rule: ConstraintRule::Existence {
                source_path: source,
                required,
            },
            expression: None,
            span: syntax.span.clone(),
        })
    }

    fn existence_path(&mut self, owner: usize, path: &ObjectPath) -> Option<(ExpressionId, usize)> {
        let (kind, value_type, ty) = self.expression_path(owner, path)?;
        if let (BoundExpressionKind::Path { steps }, Some(ty)) = (&kind, ty)
            && let Some(last) = steps.last()
            && self.symbols[self.names[last.reference.target_name()]]
                .declaration
                .kind
                == Kind::Attribute
        {
            return Some((
                self.record_expression(kind, value_type, Some(ty), &path.span),
                ty,
            ));
        }
        self.diagnostics.push(
            "ILI-EXISTENCE-PATH",
            "An EXISTENCE path must end at an attribute",
            Some(path.span.clone()),
        );
        None
    }

    fn existence_compatible(
        &mut self,
        owner: usize,
        mut actual: usize,
        mut expected: usize,
        at: &Span,
    ) -> Option<bool> {
        let singleton = |collection: &crate::CollectionType| {
            collection
                .cardinality
                .upper
                .as_deref()
                .is_some_and(|upper| compare_count(upper, "1") != Ordering::Greater)
        };
        let mut from_vertex_domain = false;
        loop {
            if !self.step(1, at) {
                return None;
            }
            if actual == expected {
                return Some(true);
            }
            match (&self.types[actual], &self.types[expected]) {
                (TypeInfo::Collection(source), TypeInfo::Collection(target)) => {
                    if source.kind != target.kind {
                        return Some(false);
                    }
                    actual = source.element;
                    expected = target.element;
                }
                (TypeInfo::Collection(source), _) if singleton(source) => actual = source.element,
                (_, TypeInfo::Collection(target)) if singleton(target) => expected = target.element,
                (TypeInfo::Geometry(geometry), TypeInfo::Coordinate(_)) => {
                    let Some(vertex) = geometry.vertex.as_ref() else {
                        return Some(false);
                    };
                    let Some(ty) = self
                        .names
                        .get(vertex.target_name())
                        .and_then(|&id| self.symbols[id].declaration.type_id)
                    else {
                        self.unsupported_expression(at);
                        return None;
                    };
                    actual = ty;
                    from_vertex_domain = true;
                }
                _ => break,
            }
        }
        let (source, target) = (&self.types[actual], &self.types[expected]);
        let compatible = match (source, target) {
            (TypeInfo::Structure(_), TypeInfo::Structure(_)) => {
                return self.structure_specializes(actual, expected, at);
            }
            (TypeInfo::Reference(_), TypeInfo::Reference(_)) => {
                return self.reference_specializes(actual, expected, at);
            }
            (TypeInfo::Numeric(source), TypeInfo::Numeric(target)) => {
                numeric_compatible(source, target)
            }
            (TypeInfo::Coordinate(source), TypeInfo::Coordinate(target)) => {
                if source.axes.len() != target.axes.len() {
                    return Some(false);
                }
                if from_vertex_domain {
                    let language = self.models[self.symbols[owner].model].language;
                    let check = self
                        .check_unit_specialization(actual, expected, false)
                        .and_then(|reason| {
                            if reason.is_some() {
                                Ok(reason)
                            } else {
                                scalar_extension(
                                    &self.types[actual],
                                    &self.types[expected],
                                    language,
                                    self.limits,
                                    &mut self.work,
                                    false,
                                )
                            }
                        });
                    return match check {
                        Ok(reason) => Some(reason.is_none()),
                        Err(error) => {
                            self.number_error(error, at);
                            None
                        }
                    };
                }
                source.kind == target.kind
                    && source.reference_system == target.reference_system
                    && source
                        .axes
                        .iter()
                        .zip(&target.axes)
                        .all(|(source, target)| numeric_compatible(source, target))
            }
            (TypeInfo::Oid(source), TypeInfo::Oid(target)) => oid_compatible(source, target),
            (TypeInfo::Blackbox(source), TypeInfo::Blackbox(target)) => source == target,
            (TypeInfo::Temporal(source), TypeInfo::Temporal(target)) => source == target,
            (TypeInfo::TemporalRange(source), TypeInfo::TemporalRange(target)) => {
                source.kind == target.kind
            }
            (TypeInfo::TemporalRange(range), TypeInfo::Temporal(kind))
            | (TypeInfo::Temporal(kind), TypeInfo::TemporalRange(range)) => range.kind == *kind,
            (source, target) if text_compatible(source, target) => true,
            _ if self.is_enum_type(actual) && self.is_enum_type(expected) => {
                return self.enum_existence_compatible(actual, expected, at);
            }
            (TypeInfo::ClassValue(_), TypeInfo::ClassValue(_)) => {
                return self.class_value_specializes(actual, expected, at, false);
            }
            (TypeInfo::AttributePath(_), TypeInfo::AttributePath(_)) => {
                return self.attribute_path_specializes(owner, actual, expected, at, None);
            }
            (TypeInfo::Geometry(_), TypeInfo::Geometry(_)) => {
                self.unsupported_expression(at);
                return None;
            }
            _ => false,
        };
        Some(compatible)
    }
}

pub(super) fn numeric_compatible(actual: &NumericRange, expected: &NumericRange) -> bool {
    actual.unit.as_ref().map(TypeReference::target_name)
        == expected.unit.as_ref().map(TypeReference::target_name)
        && actual.circular == expected.circular
        && actual.direction == expected.direction
        && actual
            .reference_system
            .as_ref()
            .map(|value| (value.domain.target_name(), value.axis))
            == expected
                .reference_system
                .as_ref()
                .map(|value| (value.domain.target_name(), value.axis))
}

fn text_compatible(actual: &TypeInfo, expected: &TypeInfo) -> bool {
    scalar_type(actual) == ExpressionType::Text && scalar_type(expected) == ExpressionType::Text
}

pub(super) fn oid_compatible(actual: &OidType, expected: &OidType) -> bool {
    match (actual, expected) {
        (OidType::Any, _)
        | (_, OidType::Any)
        | (OidType::Unstable, OidType::Unstable)
        | (OidType::Standard, OidType::Standard)
        | (OidType::Uuid, OidType::Uuid) => true,
        (OidType::Standard, OidType::Typed(carrier))
        | (OidType::Typed(carrier), OidType::Standard) => {
            scalar_type(carrier) == ExpressionType::Text
        }
        (OidType::Typed(actual), OidType::Typed(expected)) => {
            match (actual.as_ref(), expected.as_ref()) {
                (TypeInfo::Numeric(actual), TypeInfo::Numeric(expected)) => {
                    numeric_compatible(actual, expected)
                }
                (actual, expected) => text_compatible(actual, expected),
            }
        }
        _ => false,
    }
}
