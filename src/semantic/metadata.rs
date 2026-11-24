use super::{Analyzer, BoundName, Kind, Namespace, QualifiedName, SyntaxDecl};
use crate::{MetadataBasketKind, Span, TypeReference};

impl Analyzer<'_, '_> {
    pub(super) fn bind_metadata_baskets(&mut self) -> Option<()> {
        let mut baskets = Vec::new();
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::MetadataBasket(basket)) = self.symbols[id].ast else {
                continue;
            };
            let topic = self.metadata_reference(id, &basket.topic, Kind::Topic)?;
            let declaration = &mut self.symbols[id].declaration;
            declaration.metadata_kind = Some(basket.kind.value);
            declaration.reference = Some(topic.1);
            if let Some(name) = &basket.base {
                let (parent, reference) =
                    self.metadata_reference(id, name, Kind::MetadataBasket)?;
                if self.symbols[parent].declaration.is_final() {
                    return self.metadata_error(
                        "ILI-METADATA-BASKET-FINAL",
                        "A FINAL metadata basket cannot be extended",
                        &name.span,
                    );
                }
                self.symbols[id].base_scope = Some(parent);
                self.symbols[id].declaration.base_reference = Some(reference);
            }
            baskets.push(id);
        }
        for id in self.inheritance_order(baskets, "ILI-METADATA-BASKET-CYCLE")? {
            let Some(SyntaxDecl::MetadataBasket(basket)) = self.symbols[id].ast else {
                unreachable!("ordinary basket has basket syntax");
            };
            let topic = self.names[self.symbols[id].declaration.type_reference()?.target_name()];
            if let Some(parent) = self.symbols[id].base_scope {
                let parent = &self.symbols[parent].declaration;
                if parent.metadata_basket_kind() != Some(basket.kind.value) {
                    return self.metadata_error(
                        "ILI-METADATA-BASKET-KIND",
                        "A basket extension must retain its kind",
                        &basket.kind.span,
                    );
                }
                let parent_topic = self.names[parent.type_reference()?.target_name()];
                if !self.specializes_symbol(topic, parent_topic, &basket.topic.span)? {
                    return self.metadata_error(
                        "ILI-METADATA-BASKET-TOPIC",
                        "The basket topic must equal or specialize the inherited basket topic",
                        &basket.topic.span,
                    );
                }
            }
            if basket.groups.is_empty() {
                continue;
            }
            let root = match basket.kind.value {
                MetadataBasketKind::Sign => "SIGN",
                MetadataBasketKind::ReferenceSystem => "REFSYSTEM",
            };
            let root = self.predefined_metadata(id, root, &basket.kind.span)?;
            for group in &basket.groups {
                let class = &group.class;
                let Some(target) = self
                    .visible_name(topic, &class.value, Some(&[Kind::Class]), &class.span)
                    .ok()?
                else {
                    return self.metadata_error(
                        "ILI-METAOBJECT-CLASS",
                        "The object class must be visible in the basket topic",
                        &class.span,
                    );
                };
                if !self.specializes_symbol(target, root, &class.span)? {
                    return self.metadata_error(
                        "ILI-METAOBJECT-CLASS-KIND",
                        "The object class must match the metadata basket family",
                        &class.span,
                    );
                }
                for object in &group.objects {
                    let obj = self.scope_names[&(id, Namespace::MetaObject, object.value.clone())];
                    let reference = self.reference_to_symbol(obj, target, class)?;
                    self.symbols[obj].declaration.reference = Some(reference);
                }
            }
        }
        Some(())
    }

    fn metadata_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kind: Kind,
    ) -> Option<(usize, TypeReference)> {
        let target = self.expression_name(owner, name, &[kind], "ILI-METADATA-NAME")?;
        let BoundName::Declared(id) = target else {
            self.unsupported_expression(&name.span);
            return None;
        };
        Some((id, self.bound_reference(owner, name, target)?))
    }

    fn metadata_error<T>(&mut self, code: &str, message: &str, at: &Span) -> Option<T> {
        self.diagnostics.push(code, message, Some(at.clone()));
        None
    }
}
