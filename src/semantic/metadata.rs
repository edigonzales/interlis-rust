use super::{
    Analyzer, BoundName, Effective, Kind, LookupError, Namespace, QualifiedName, SyntaxDecl,
};
use crate::{
    MetadataBasketKind, Span, TypeInfo, TypeReference,
    syntax::{ParameterType, Property},
};

impl Analyzer<'_, '_> {
    pub(super) fn bind_metadata_parameters(&mut self, viewables: &[usize]) -> Option<()> {
        for &owner in viewables {
            let Some(SyntaxDecl::Container { parameters, .. }) = self.symbols[owner].ast else {
                continue;
            };
            for parameter in parameters {
                let name = &parameter.name;
                let id = self.scope_names[&(owner, Namespace::Member, name.value.clone())];
                let sign = self.specializes_metadata_family(owner, "INTERLIS.SIGN", &name.span)?;
                let base = if let Some(parent) = self.symbols[owner].base_scope {
                    self.visible_name(parent, &name.value, Some(&[Kind::Parameter]), &name.span)
                        .ok()?
                } else {
                    None
                };
                if !sign {
                    if !self.specializes_metadata_family(owner, "INTERLIS.AXIS", &name.span)?
                        && !self.specializes_metadata_family(
                            owner,
                            "INTERLIS.REFSYSTEM",
                            &name.span,
                        )?
                    {
                        return self.metadata_error("ILI-PARAMETER-OWNER", "Class parameters require a signature, axis or reference-system declaration", &name.span);
                    }
                    if name.value != "Unit" || base.is_none() {
                        return self.metadata_error("ILI-PARAMETER-UNIT", "An axis or reference system can only refine its inherited Unit parameter", &name.span);
                    }
                }
                let extended = parameter
                    .properties
                    .iter()
                    .any(|p| p.value == Property::Extended);
                if extended != base.is_some() {
                    return self.metadata_error(
                        "ILI-PARAMETER-OVERRIDE",
                        "A parameter override requires EXTENDED and an inherited parameter",
                        &name.span,
                    );
                }
                let inherited = if let Some(base) = base {
                    let decl = &self.symbols[base].declaration;
                    if decl.is_final() {
                        return self.metadata_error(
                            "ILI-PARAMETER-FINAL",
                            "A FINAL parameter cannot be overridden",
                            &name.span,
                        );
                    }
                    let ty = decl.type_id?;
                    let mandatory = decl.mandatory() == Some(true);
                    self.symbols[id].base_scope = Some(base);
                    self.symbols[id].declaration.base_reference =
                        Some(self.reference_to_symbol(id, base, name)?);
                    Some(Effective { ty, mandatory })
                } else {
                    None
                };
                let (ty, mandatory) = match &parameter.ty {
                    ParameterType::Attribute(syntax) => {
                        self.symbols[id].declaration.declared_mandatory = Some(syntax.mandatory);
                        let effective = match (&syntax.ty, inherited) {
                            (Some(ty), Some(base)) => self.specialize_scalar(id, base, ty)?,
                            (Some(ty), None) => self.concrete_type(id, ty)?,
                            (None, Some(base)) => base,
                            (None, None) => {
                                return self.metadata_error(
                                    "ILI-PARAMETER-TYPE",
                                    "The parameter needs a declared or inherited type",
                                    &parameter.span,
                                );
                            }
                        };
                        (effective.ty, effective.mandatory || syntax.mandatory)
                    }
                    ParameterType::Metaobject(class) if sign => {
                        let root = self.predefined_metadata(id, "METAOBJECT", &name.span)?;
                        let reference = if let Some(class) = class {
                            let bound = self.expression_name(
                                id,
                                class,
                                &[Kind::Class],
                                "ILI-PARAMETER-METAOBJECT",
                            )?;
                            let class_id = match bound {
                                BoundName::Declared(id) => id,
                                BoundName::Predefined(builtin, _) => {
                                    let Some(id) =
                                        self.predefined_metadata(id, builtin, &class.span)
                                    else {
                                        self.unsupported_expression(&class.span);
                                        return None;
                                    };
                                    id
                                }
                            };
                            if !self.specializes_symbol(class_id, root, &class.span)? {
                                return self.metadata_error(
                                    "ILI-PARAMETER-METAOBJECT",
                                    "The parameter type must specialize METAOBJECT",
                                    &class.span,
                                );
                            }
                            Some(self.bound_reference(id, class, bound)?)
                        } else {
                            None
                        };
                        if let Some(inherited) = inherited {
                            let TypeInfo::MetaObject(base) = &self.types[inherited.ty] else {
                                return self.metadata_error(
                                    "ILI-PARAMETER-TYPE",
                                    "An override must retain the parameter type family",
                                    &parameter.span,
                                );
                            };
                            if let Some(base) = base {
                                let valid = if let Some(reference) = &reference {
                                    self.specializes_symbol(
                                        self.names[reference.target_name()],
                                        self.names[base.target_name()],
                                        &parameter.span,
                                    )?
                                } else {
                                    false
                                };
                                if !valid {
                                    return self.metadata_error("ILI-PARAMETER-TYPE", "A metadata parameter override must specialize the inherited class", &parameter.span);
                                }
                            }
                        }
                        let ty = self.types.len();
                        self.types.push(TypeInfo::MetaObject(reference));
                        (ty, false)
                    }
                    _ => {
                        return self.metadata_error(
                            "ILI-PARAMETER-UNIT",
                            "An axis or reference-system Unit parameter must be numeric",
                            &parameter.span,
                        );
                    }
                };
                if !sign
                    && !matches!(&self.types[ty], TypeInfo::Numeric(range) if range.unit.is_some())
                {
                    return self.metadata_error(
                        "ILI-PARAMETER-UNIT",
                        "A Unit parameter requires a numeric type with a unit",
                        &parameter.span,
                    );
                }
                self.symbols[id].declaration.type_id = Some(ty);
                self.symbols[id].declaration.effective_mandatory = Some(mandatory);
            }
        }
        Some(())
    }

    pub(super) fn specializes_metadata_family(
        &mut self,
        candidate: usize,
        family: &str,
        at: &Span,
    ) -> Option<bool> {
        if let Some(&base) = self.names.get(family) {
            self.specializes_symbol(candidate, base, at)
        } else {
            Some(false)
        }
    }

    pub(super) fn bind_metadata_baskets(&mut self) -> Option<()> {
        let mut baskets = Vec::new();
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::MetadataBasket(basket)) = self.symbols[id].ast else {
                continue;
            };
            if basket.kind.value == MetadataBasketKind::Sign
                && self.models[self.symbols[id].model].kind() == crate::ModelKind::Type
            {
                return self.metadata_error(
                    "ILI-TYPE-MODEL-METADATA",
                    "A TYPE MODEL can only declare reference-system baskets",
                    &basket.name.span,
                );
            }
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
                continue;
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
        let id = match target {
            BoundName::Declared(id) => id,
            BoundName::Predefined("TIMESYSTEMS", Kind::Topic) => {
                self.predefined_time_topic(owner, &name.span)?
            }
            BoundName::Predefined("BaseTimeSystems", Kind::MetadataBasket) => {
                self.predefined_time_basket(owner, &name.span)?
            }
            _ => {
                self.unsupported_expression(&name.span);
                return None;
            }
        };
        Some((id, self.bound_reference(owner, name, target)?))
    }

    pub(super) fn metaobject_reference(
        &mut self,
        owner: usize,
        reference: &QualifiedName,
    ) -> Option<TypeReference> {
        let (name, prefix) = reference.segments.split_last()?;
        let basket = if prefix.is_empty() {
            self.implicit_metadata_basket(owner, &reference.span)?
        } else {
            match self.lookup_name(owner, prefix, &reference.span, &[Kind::MetadataBasket]) {
                Ok(BoundName::Declared(id)) => Some(id),
                Ok(BoundName::Predefined("BaseTimeSystems", Kind::MetadataBasket)) => {
                    Some(self.predefined_time_basket(owner, &reference.span)?)
                }
                Ok(BoundName::Predefined(..)) => {
                    self.unsupported_expression(&reference.span);
                    return None;
                }
                Err(LookupError::Limit) => return None,
                Err(_) => None,
            }
        };
        let Some(basket) = basket else {
            return self.metadata_error(
                "ILI-METAOBJECT-REFERENCE",
                "The metadata reference requires an unambiguous basket",
                &reference.span,
            );
        };
        let Some(target) = self
            .visible_name(basket, &name.value, Some(&[Kind::MetaObject]), &name.span)
            .ok()?
        else {
            return self.metadata_error(
                "ILI-METAOBJECT-REFERENCE",
                "The named object is absent from the metadata basket and its bases",
                &name.span,
            );
        };
        self.bound_reference(owner, reference, BoundName::Declared(target))
    }

    fn implicit_metadata_basket(&mut self, owner: usize, at: &Span) -> Option<Option<usize>> {
        let mut scope = Some(owner);
        while let Some(id) = scope {
            let members = || {
                self.scope_names
                    .range((id, Namespace::Type, String::new())..)
                    .take_while(|((scope, namespace, _), _)| {
                        *scope == id && *namespace == Namespace::Type
                    })
            };
            if !self.step(1 + members().count(), at) {
                return None;
            }
            let mut baskets = self
                .scope_names
                .range((id, Namespace::Type, String::new())..)
                .take_while(|((scope, namespace, _), _)| {
                    *scope == id && *namespace == Namespace::Type
                })
                .filter_map(|(_, &member)| {
                    (self.symbols[member].declaration.kind == Kind::MetadataBasket)
                        .then_some(member)
                });
            if let Some(basket) = baskets.next() {
                return Some(baskets.next().is_none().then_some(basket));
            }
            scope = self.symbols[id].declaration.parent;
        }
        Some(None)
    }

    fn metadata_error<T>(&mut self, code: &str, message: &str, at: &Span) -> Option<T> {
        self.diagnostics.push(code, message, Some(at.clone()));
        None
    }
}
