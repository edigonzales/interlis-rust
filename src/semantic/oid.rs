use super::{Analyzer, BoundName, Kind, LookupError, QualifiedName, SyntaxDecl};
use crate::{OidKind, OidRule, OidRules, OidType, TypeInfo, syntax};

impl Analyzer<'_, '_> {
    pub(super) fn bind_oid_rules(&mut self) -> Option<()> {
        for id in 0..self.symbols.len() {
            let rules = match self.symbols[id].ast {
                Some(SyntaxDecl::Topic { options, .. }) => OidRules {
                    basket: match &options.basket_oid {
                        Some(name) => Some(self.bind_oid_domain(id, name)?),
                        None => None,
                    },
                    object: match &options.oid {
                        Some(name) => Some(self.bind_oid_domain(id, name)?),
                        None => None,
                    },
                },
                Some(SyntaxDecl::Container {
                    kind: syntax::ContainerKind::Class,
                    oid,
                    ..
                }) => {
                    let object = match oid {
                        syntax::OidRule::None(span) => Some(OidRule {
                            kind: OidKind::Unstable,
                            domain: None,
                            span: span.clone(),
                        }),
                        syntax::OidRule::Domain(name) => Some(self.bind_oid_domain(id, name)?),
                        syntax::OidRule::Unspecified => self.topic_oid(id)?,
                    };
                    OidRules {
                        basket: None,
                        object,
                    }
                }
                _ => continue,
            };
            if rules.object.is_none() && rules.basket.is_none() {
                continue;
            }
            let declaration = &self.symbols[id].declaration;
            if !declaration.is_abstract()
                && let Some(rule) = rules
                    .object
                    .iter()
                    .chain(rules.basket.iter())
                    .find(|rule| rule.kind == OidKind::Any)
            {
                self.diagnostics.push(
                    "ILI-OID-ABSTRACT",
                    "An open OID rule requires an abstract declaration",
                    Some(rule.span.clone()),
                );
                return None;
            }
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
            BoundName::Predefined(name_value, _) => self.predefined_type(name_value, &name.span),
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

    fn topic_oid(&mut self, owner: usize) -> Option<Option<OidRule>> {
        let parent = self.symbols[owner].declaration.parent?;
        let object = self.symbols[parent]
            .declaration
            .oid_rules()
            .and_then(|rule| rule.object.as_ref());
        if let Some(rule) = object {
            let bytes = rule.domain.as_ref().map_or(0, |domain| {
                domain.declared_name().len() + domain.target_name().len()
            });
            let span = rule.span.clone();
            if !self.step(bytes, &span) {
                return None;
            }
        }
        Some(
            self.symbols[parent]
                .declaration
                .oid_rules()
                .and_then(|rule| rule.object.clone()),
        )
    }
}
