use super::{
    Analyzer, BoundName, Kind, LookupError, QualifiedName, SyntaxDecl, collections::compare_count,
    existence::numeric_compatible, functions::compare_decimal,
};
use crate::{
    LanguageVersion, OidKind, OidRule, OidRules, OidType, RelatedLocation, TypeInfo,
    limits::WorkBudget, number::NumberError, syntax,
};
use std::cmp::Ordering;

pub(super) fn oid_value_fits(
    actual: &OidType,
    expected: &OidType,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    use OidType::*;
    Ok(match (actual, expected) {
        (_, Any) | (Unstable, Unstable) | (Standard, Standard) | (Uuid, Uuid) => true,
        (Standard, Typed(carrier)) => matches!(carrier.as_ref(), TypeInfo::Text { max_length }
            if compare_count("16", max_length) != Ordering::Greater),
        (Typed(actual), Typed(expected)) => match (actual.as_ref(), expected.as_ref()) {
            (TypeInfo::Numeric(actual), TypeInfo::Numeric(expected)) => {
                numeric_compatible(actual, expected)
                    && match (&actual.bounds, &expected.bounds) {
                        (_, None) => true,
                        (Some(actual), Some(expected)) => {
                            compare_decimal(&actual.lower, &expected.lower, work)? != Ordering::Less
                                && compare_decimal(&actual.upper, &expected.upper, work)?
                                    != Ordering::Greater
                        }
                        (None, Some(_)) => false,
                    }
            }
            (
                TypeInfo::Text { max_length: actual },
                TypeInfo::Text {
                    max_length: expected,
                },
            ) => compare_count(actual, expected) != Ordering::Greater,
            _ => false,
        },
        _ => false,
    })
}

impl Analyzer<'_, '_> {
    pub(super) fn bind_oid_rules(&mut self, topics: &[usize], viewables: &[usize]) -> Option<()> {
        for &id in topics {
            let Some(SyntaxDecl::Topic { options, .. }) = self.symbols[id].ast else {
                unreachable!("topic ordering only contains topics");
            };
            let base = self.symbols[id].base_scope;
            let basket = self.topic_rule(id, base, options.basket_oid.as_ref(), true)?;
            let object = self.topic_rule(id, base, options.oid.as_ref(), false)?;
            self.store_oid_rules(id, OidRules { basket, object })?;
        }
        for &id in viewables {
            let oid = match self.symbols[id].ast {
                Some(SyntaxDecl::Container {
                    kind: syntax::ContainerKind::Class,
                    oid,
                    ..
                }) => oid,
                Some(SyntaxDecl::Association(association)) => &association.oid,
                _ => continue,
            };
            let class_base = self.symbols[id].base_scope.filter(|&base| {
                self.symbols[base].declaration.kind == self.symbols[id].declaration.kind
            });
            let object = match oid {
                syntax::OidRule::None(span) => Some(OidRule {
                    kind: OidKind::Unstable,
                    domain: None,
                    span: span.clone(),
                }),
                syntax::OidRule::Domain(name) => Some(self.bind_oid_domain(id, name)?),
                syntax::OidRule::Unspecified if self.existing_oid(class_base, false).is_some() => {
                    self.inherit_oid(class_base, false)?
                }
                syntax::OidRule::Unspecified => {
                    self.inherit_oid(self.symbols[id].declaration.parent, false)?
                }
            };
            if let (Some(base), Some(own)) = (class_base, &object) {
                let previous = self.existing_oid(Some(base), false);
                let language = self.models[self.symbols[id].model].language;
                if !class_oid_specializes(previous, own, language) {
                    self.diagnostics.push_related(
                        "ILI-OID-OVERRIDE",
                        "This OID rule cannot specialize the base class OID",
                        Some(own.span.clone()),
                        vec![RelatedLocation {
                            span: previous.map_or_else(
                                || self.symbols[base].name_span.clone(),
                                |rule| rule.span.clone(),
                            ),
                            message: "Inherited OID rule".into(),
                        }],
                    );
                    return None;
                }
            }
            self.store_oid_rules(
                id,
                OidRules {
                    basket: None,
                    object,
                },
            )?;
        }
        Some(())
    }

    fn topic_rule(
        &mut self,
        owner: usize,
        base: Option<usize>,
        name: Option<&QualifiedName>,
        basket: bool,
    ) -> Option<Option<OidRule>> {
        if let Some(name) = name {
            let rule = self.bind_oid_domain(owner, name)?;
            if let Some(inherited) = self.existing_oid(base, basket)
                && !same_oid_domain(inherited, &rule)
            {
                self.diagnostics.push(
                    "ILI-OID-OVERRIDE",
                    "An extending topic cannot change its inherited OID domain",
                    Some(name.span.clone()),
                );
                return None;
            }
            Some(Some(rule))
        } else {
            self.inherit_oid(base, basket)
        }
    }

    fn store_oid_rules(&mut self, id: usize, rules: OidRules) -> Option<()> {
        if !self.symbols[id].declaration.is_abstract()
            && let Some(rule) = rules
                .object
                .iter()
                .chain(rules.basket.iter())
                .find(|rule| rule.kind == OidKind::Any)
        {
            self.diagnostics.push_related(
                "ILI-OID-ABSTRACT",
                "An open OID rule requires an abstract declaration",
                Some(self.symbols[id].name_span.clone()),
                vec![RelatedLocation {
                    span: rule.span.clone(),
                    message: "Open OID rule".into(),
                }],
            );
            return None;
        }
        if rules.object.is_some() || rules.basket.is_some() {
            self.symbols[id].declaration.oids = Some(Box::new(rules));
        }
        Some(())
    }

    fn bind_oid_domain(&mut self, owner: usize, name: &QualifiedName) -> Option<OidRule> {
        let target = match self.lookup_name(owner, &name.segments, &name.span, &[Kind::Domain]) {
            Ok(target) => target,
            Err(LookupError::Limit) => return None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "OID domain reference cannot be resolved",
                    Some(name.span.clone()),
                );
                return None;
            }
        };
        let effective = match target {
            BoundName::Declared(id) => self.resolve_domain(id),
            BoundName::Predefined(name_value, _) => {
                self.predefined_type(owner, name_value, &name.span)
            }
        }?;
        let kind = match &self.types[effective.ty] {
            TypeInfo::Oid(OidType::Unstable) => OidKind::Unstable,
            TypeInfo::Oid(OidType::Any) => OidKind::Any,
            TypeInfo::Oid(carrier) => {
                if matches!(carrier, OidType::Typed(ty) if ty.has_unbounded_range()) {
                    OidKind::Any
                } else {
                    OidKind::Concrete
                }
            }
            _ => {
                self.diagnostics.push(
                    "ILI-OID-DOMAIN",
                    "An OID rule must name an OID domain",
                    Some(name.span.clone()),
                );
                return None;
            }
        };
        Some(OidRule {
            kind,
            domain: Some(self.bound_reference(owner, name, target)?),
            span: name.span.clone(),
        })
    }

    fn existing_oid(&self, owner: Option<usize>, basket: bool) -> Option<&OidRule> {
        let rules = self.symbols[owner?].declaration.oid_rules()?;
        if basket {
            rules.basket.as_ref()
        } else {
            rules.object.as_ref()
        }
    }

    fn inherit_oid(&mut self, owner: Option<usize>, basket: bool) -> Option<Option<OidRule>> {
        if let Some(rule) = self.existing_oid(owner, basket) {
            let bytes = rule.domain.as_ref().map_or(0, |domain| {
                domain.declared_name().len() + domain.target_name().len()
            });
            let span = rule.span.clone();
            if !self.step(bytes, &span) {
                return None;
            }
        }
        Some(self.existing_oid(owner, basket).cloned())
    }
}

fn class_oid_specializes(
    previous: Option<&OidRule>,
    own: &OidRule,
    language: LanguageVersion,
) -> bool {
    let Some(previous) = previous else {
        return language == LanguageVersion::V23 || own.kind != OidKind::Concrete;
    };
    if same_oid_domain(previous, own) {
        return true;
    }
    matches!(
        (previous.kind, own.kind),
        (OidKind::Any, OidKind::Concrete) | (OidKind::Unstable, OidKind::Unstable)
    ) || language == LanguageVersion::V24
        && previous.kind == OidKind::Unstable
        && own.kind == OidKind::Any
}

fn same_oid_domain(left: &OidRule, right: &OidRule) -> bool {
    match (&left.domain, &right.domain) {
        (Some(left), Some(right)) => {
            left.target_name() == right.target_name()
                && left.predefined_language() == right.predefined_language()
        }
        (None, None) => true,
        _ => false,
    }
}
