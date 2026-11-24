use super::{Analyzer, BoundName, Kind, Name};
use crate::{
    BoundConstraint, BoundExpressionKind, BoundPathStep, Cardinality, CollectionKind,
    CollectionType, ConstraintRule, ExpressionType, NumericRange, Span, StructureType, TypeInfo,
    TypeReference,
};

impl Analyzer<'_, '_> {
    pub(super) fn predefined_segment(
        &mut self,
        owner: usize,
        name: &str,
        at: &Span,
    ) -> Option<usize> {
        let (base, is_final) = match name {
            "LineSegment" => (None, false),
            "StartSegment" | "StraightSegment" | "ArcSegment" => (Some("LineSegment"), true),
            _ => return None,
        };
        let qualified = format!("INTERLIS.{name}");
        if let Some(&id) = self.names.get(&qualified) {
            return Some(id);
        }
        let base = if let Some(base) = base {
            Some(self.predefined_segment(owner, base, at)?)
        } else {
            None
        };
        let model = self.symbols[owner].model;
        let namespace = self.predefined_namespace(model)?;
        let declared = Name {
            value: name.into(),
            span: Span::predefined(),
        };
        let id = self.add(
            Kind::Structure,
            &declared,
            &declared.span,
            Some(namespace),
            model,
            None,
        )?;
        self.symbols[id].declaration.predefined = true;
        self.symbols[id].declaration.abstract_declaration = base.is_none();
        self.symbols[id].declaration.final_declaration = is_final;
        let mut members = if let Some(base) = base {
            let name = Name {
                value: "INTERLIS.LineSegment".into(),
                span: Span::predefined(),
            };
            let reference = self.reference_to_symbol(owner, base, &name)?;
            self.symbols[id].base_scope = Some(base);
            self.symbols[id].declaration.base_reference = Some(reference);
            self.symbols[base]
                .declaration
                .effective_attributes
                .as_deref()
                .unwrap_or_default()
                .to_vec()
        } else {
            let ty = self.predefined_type(owner, "LineCoord", at)?.ty;
            let reference = self.predefined_reference(owner, "LineCoord");
            vec![self.predefined_attribute(id, "SegmentEndPoint", ty, true, Some(reference))?]
        };
        if name == "ArcSegment" {
            let ty = self.predefined_type(owner, "LineCoord", at)?.ty;
            let reference = self.predefined_reference(owner, "LineCoord");
            members.push(self.predefined_attribute(id, "ArcPoint", ty, true, Some(reference))?);
            let radius = self.types.len();
            self.types.push(TypeInfo::Numeric(NumericRange {
                bounds: None,
                unit: Some(self.predefined_reference(owner, "LENGTH")),
                reference_system: None,
                circular: false,
                direction: None,
            }));
            members.push(self.predefined_attribute(id, "Radius", radius, false, None)?);
        }
        self.symbols[id].declaration.effective_attributes = Some(members.into_boxed_slice());
        Some(id)
    }

    fn predefined_namespace(&mut self, model: usize) -> Option<usize> {
        if let Some(&id) = self.names.get("INTERLIS") {
            return Some(id);
        }
        let name = Name {
            value: "INTERLIS".into(),
            span: Span::predefined(),
        };
        let id = self.add(Kind::Model, &name, &name.span, None, model, None)?;
        self.symbols[id].declaration.predefined = true;
        Some(id)
    }

    fn predefined_attribute(
        &mut self,
        owner: usize,
        name: &str,
        ty: usize,
        mandatory: bool,
        reference: Option<TypeReference>,
    ) -> Option<usize> {
        self.predefined_member(owner, Kind::Attribute, name, ty, mandatory, reference)
    }

    fn predefined_member(
        &mut self,
        owner: usize,
        kind: Kind,
        name: &str,
        ty: usize,
        mandatory: bool,
        reference: Option<TypeReference>,
    ) -> Option<usize> {
        let name = Name {
            value: name.into(),
            span: Span::predefined(),
        };
        let id = self.add(
            kind,
            &name,
            &name.span,
            Some(owner),
            self.symbols[owner].model,
            Some(mandatory),
        )?;
        let decl = &mut self.symbols[id].declaration;
        decl.predefined = true;
        decl.type_id = Some(ty);
        decl.effective_mandatory = Some(mandatory);
        decl.reference = reference;
        Some(id)
    }

    pub(super) fn predefined_metadata(
        &mut self,
        owner: usize,
        name: &str,
        at: &Span,
    ) -> Option<usize> {
        if matches!(name, "TIMESYSTEMS.CALENDAR" | "TIMESYSTEMS.TIMEOFDAYSYS") {
            self.predefined_time_topic(owner, at)?;
            return self.names.get(&format!("INTERLIS.{name}")).copied();
        }
        let (kind, base, abstract_declaration) = match name {
            "AXIS" => (Kind::Structure, None, false),
            "METAOBJECT" => (Kind::Class, None, true),
            "METAOBJECT_TRANSLATION" => (Kind::Class, None, false),
            "REFSYSTEM" | "SIGN" => (Kind::Class, Some("METAOBJECT"), true),
            "COORDSYSTEM" | "SCALSYSTEM" => (Kind::Class, Some("REFSYSTEM"), true),
            _ => return None,
        };
        let qualified = format!("INTERLIS.{name}");
        if let Some(&id) = self.names.get(&qualified) {
            return Some(id);
        }
        let parent = if let Some(base) = base {
            Some((
                self.predefined_metadata(owner, base, at)?,
                self.predefined_reference(owner, base),
            ))
        } else {
            None
        };
        let model = self.symbols[owner].model;
        let namespace = self.predefined_namespace(model)?;
        let declaration = Name {
            value: name.into(),
            span: Span::predefined(),
        };
        let id = self.add(
            kind,
            &declaration,
            &declaration.span,
            Some(namespace),
            model,
            None,
        )?;
        self.symbols[id].declaration.predefined = true;
        self.symbols[id].declaration.abstract_declaration = abstract_declaration;
        let mut members = Vec::new();
        if let Some((parent, reference)) = parent {
            self.symbols[id].base_scope = Some(parent);
            self.symbols[id].declaration.base_reference = Some(reference);
            members.extend_from_slice(
                self.symbols[parent]
                    .declaration
                    .effective_attributes
                    .as_deref()
                    .unwrap_or_default(),
            );
        }
        match name {
            "METAOBJECT" | "METAOBJECT_TRANSLATION" => {
                let ty = self.predefined_type(owner, "NAME", at)?.ty;
                let attributes = if name == "METAOBJECT" {
                    &["Name"][..]
                } else {
                    &["Name", "NameInBaseLanguage"][..]
                };
                let mut constraints = Vec::new();
                for &attribute in attributes {
                    let reference = self.predefined_reference(owner, "NAME");
                    let member =
                        self.predefined_attribute(id, attribute, ty, true, Some(reference))?;
                    let name = Name {
                        value: attribute.into(),
                        span: Span::predefined(),
                    };
                    let reference = self.reference_to_symbol(owner, member, &name)?;
                    let expr = self.record_expression(
                        BoundExpressionKind::Path {
                            steps: vec![BoundPathStep {
                                reference,
                                selection: None,
                                selection_span: None,
                            }],
                        },
                        ExpressionType::Text,
                        Some(ty),
                        &name.span,
                    );
                    constraints.push(BoundConstraint {
                        name: None,
                        expression: None,
                        span: name.span,
                        rule: ConstraintRule::Unique {
                            basket: false,
                            local_prefix: None,
                            keys: vec![expr],
                        },
                    });
                    members.push(member);
                }
                self.symbols[id].declaration.constraints = Some(constraints.into_boxed_slice());
            }
            "COORDSYSTEM" => {
                self.predefined_metadata(owner, "AXIS", at)?;
                let element = self.types.len();
                self.types.push(TypeInfo::Structure(StructureType {
                    target: Some(self.predefined_reference(owner, "AXIS")),
                    restrictions: Vec::new(),
                }));
                let ty = self.types.len();
                self.types.push(TypeInfo::Collection(CollectionType {
                    kind: CollectionKind::List,
                    cardinality: Cardinality {
                        lower: "1".into(),
                        upper: Some("3".into()),
                    },
                    element,
                }));
                members.push(self.predefined_attribute(id, "Axis", ty, false, None)?);
            }
            "AXIS" | "SCALSYSTEM" => {
                let ty = self.types.len();
                self.types.push(TypeInfo::Numeric(NumericRange {
                    bounds: None,
                    unit: Some(self.predefined_reference(owner, "ANYUNIT")),
                    reference_system: None,
                    circular: false,
                    direction: None,
                }));
                self.predefined_member(id, Kind::Parameter, "Unit", ty, false, None)?;
            }
            "SIGN" => {
                let ty = self.types.len();
                self.types.push(TypeInfo::MetaObject(None));
                self.predefined_member(id, Kind::Parameter, "Sign", ty, false, None)?;
            }
            _ => {}
        }
        self.symbols[id].declaration.effective_attributes = Some(members.into_boxed_slice());
        Some(id)
    }

    pub(super) fn predefined_time_topic(&mut self, owner: usize, at: &Span) -> Option<usize> {
        if let Some(&id) = self.names.get("INTERLIS.TIMESYSTEMS") {
            return Some(id);
        }
        let model = self.symbols[owner].model;
        let namespace = self.predefined_namespace(model)?;
        let name = Name {
            value: "TIMESYSTEMS".into(),
            span: Span::predefined(),
        };
        let topic = self.add(Kind::Topic, &name, &name.span, Some(namespace), model, None)?;
        self.symbols[topic].declaration.predefined = true;
        let base = self.predefined_metadata(owner, "SCALSYSTEM", at)?;
        let unit = self
            .visible_name(base, "Unit", Some(&[Kind::Parameter]), at)
            .ok()??;
        for name in ["CALENDAR", "TIMEOFDAYSYS"] {
            let name = Name {
                value: name.into(),
                span: Span::predefined(),
            };
            let id = self.add(Kind::Class, &name, &name.span, Some(topic), model, None)?;
            self.symbols[id].base_scope = Some(base);
            self.symbols[id].declaration.predefined = true;
            self.symbols[id].declaration.base_reference =
                Some(self.predefined_reference(owner, "SCALSYSTEM"));
            self.symbols[id].declaration.effective_attributes =
                self.symbols[base].declaration.effective_attributes.clone();
            let ty = self.types.len();
            self.types.push(TypeInfo::Numeric(NumericRange {
                bounds: None,
                unit: Some(self.predefined_reference(owner, "TIME")),
                reference_system: None,
                circular: false,
                direction: None,
            }));
            let parameter = self.predefined_member(id, Kind::Parameter, "Unit", ty, false, None)?;
            let name = Name {
                value: "Unit".into(),
                span: Span::predefined(),
            };
            self.symbols[parameter].base_scope = Some(unit);
            self.symbols[parameter].declaration.base_reference =
                Some(self.reference_to_symbol(owner, unit, &name)?);
        }
        Some(topic)
    }

    pub(super) fn predefined_time_basket(&mut self, owner: usize, at: &Span) -> Option<usize> {
        if let Some(&id) = self.names.get("INTERLIS.BaseTimeSystems") {
            return Some(id);
        }
        self.predefined_time_topic(owner, at)?;
        let model = self.symbols[owner].model;
        let namespace = self.predefined_namespace(model)?;
        let name = Name {
            value: "BaseTimeSystems".into(),
            span: Span::predefined(),
        };
        let id = self.add(
            Kind::MetadataBasket,
            &name,
            &name.span,
            Some(namespace),
            model,
            None,
        )?;
        self.symbols[id].declaration.predefined = true;
        self.symbols[id].declaration.metadata_kind =
            Some(crate::MetadataBasketKind::ReferenceSystem);
        self.symbols[id].declaration.reference =
            Some(self.predefined_reference(owner, "TIMESYSTEMS"));
        for (name, class) in [
            ("GregorianCalendar", "TIMESYSTEMS.CALENDAR"),
            ("UTC", "TIMESYSTEMS.TIMEOFDAYSYS"),
        ] {
            let name = Name {
                value: name.into(),
                span: Span::predefined(),
            };
            let object = self.add(Kind::MetaObject, &name, &name.span, Some(id), model, None)?;
            self.symbols[object].declaration.predefined = true;
            self.symbols[object].declaration.reference =
                Some(self.predefined_reference(owner, class));
        }
        Some(id)
    }

    pub(super) fn predefined_reference(&self, owner: usize, name: &str) -> TypeReference {
        TypeReference {
            declared: name.into(),
            target: format!("INTERLIS.{name}"),
            predefined_language: Some(self.models[self.symbols[owner].model].language()),
            span: Span::predefined(),
        }
    }

    pub(super) fn bound_structure(
        &mut self,
        owner: usize,
        bound: BoundName,
        at: &Span,
    ) -> Option<usize> {
        match bound {
            BoundName::Declared(id) => Some(id),
            BoundName::Predefined(name, Kind::Structure) => {
                if let Some(id) = self
                    .predefined_segment(owner, name, at)
                    .or_else(|| self.predefined_metadata(owner, name, at))
                {
                    Some(id)
                } else {
                    if !self.diagnostics.limited {
                        self.diagnostics.push(
                            "ILI-UNSUPPORTED",
                            "This predefined structure requires its full declaration",
                            Some(at.clone()),
                        );
                    }
                    None
                }
            }
            _ => unreachable!("bound structure target has a structure kind"),
        }
    }
}
