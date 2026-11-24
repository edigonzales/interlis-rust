use super::{Analyzer, BoundName, Kind, Name};
use crate::{
    BoundConstraint, BoundExpressionKind, BoundPathStep, Cardinality, CollectionKind,
    CollectionType, ConstraintRule, ExpressionType, GeometryKind, GeometryType, LanguageVersion,
    NumericBounds, NumericRange, NumericReferenceSystem, Span, StructureType, TypeInfo,
    TypeReference,
};

impl Analyzer<'_, '_> {
    pub(super) fn materialized_class_value(
        &mut self,
        owner: usize,
        target: BoundName,
        at: &Span,
    ) -> Option<BoundName> {
        if let BoundName::Predefined(
            "SurfaceEdge" | "SurfaceBoundary" | "LineGeometry" | "TimeOfDay" | "UTC"
            | "GregorianDate" | "GregorianDateTime",
            Kind::Structure,
        ) = target
        {
            return Some(BoundName::Declared(
                self.bound_structure(owner, target, at)?,
            ));
        }
        Some(target)
    }

    pub(super) fn predefined_geometry_structure(
        &mut self,
        owner: usize,
        family: &str,
        at: &Span,
    ) -> Option<usize> {
        let language = self.expression_language(owner);
        let standard = format!("INTERLIS.{family}");
        let versioned = match family {
            "LineGeometry" => family.to_owned(),
            "SurfaceEdge" | "SurfaceBoundary" => format!(
                "{family}@{}",
                if language == LanguageVersion::V23 {
                    "2.3"
                } else {
                    "2.4"
                }
            ),
            _ => return None,
        };
        let qualified = format!("INTERLIS.{versioned}");
        if let Some(&id) = self.names.get(&qualified) {
            return Some(id);
        }
        let template = self.predefined_namespace(self.symbols[owner].model)?;
        let id = self.geometry_structure(template, &versioned, family, None)?;
        self.names.entry(standard).or_insert(id);
        let mut members = Vec::new();
        if family == "SurfaceEdge" {
            let ty = self.types.len();
            self.types.push(TypeInfo::Geometry(GeometryType {
                geometry_kind: GeometryKind::Polyline,
                directed: true,
                line_forms: Vec::new(),
                vertex: None,
                without_overlaps: false,
                tolerance: None,
                line_attributes: None,
                language,
                span: Span::predefined(),
                base_vertex: None,
                explicit_tolerance: false,
            }));
            members.push(self.predefined_attribute(id, "Geometry", ty, false, None)?);
            if language == LanguageVersion::V23 {
                let ty = self.structure_type_id(None);
                members.push(self.predefined_attribute(id, "LineAttrs", ty, false, None)?);
            }
        } else {
            let (attribute, element) = if family == "LineGeometry" {
                (
                    "Segments",
                    self.predefined_segment(owner, "LineSegment", at)?,
                )
            } else {
                (
                    "Lines",
                    self.predefined_geometry_structure(owner, "SurfaceEdge", at)?,
                )
            };
            let ty = self.geometry_structure_list(owner, element)?;
            let member = self.predefined_attribute(id, attribute, ty, false, None)?;
            members.push(member);
            if family == "LineGeometry" {
                let start = self.predefined_segment(owner, "StartSegment", at)?;
                self.geometry_start_constraint(owner, id, member, element, start)?;
            }
        }
        self.symbols[id].declaration.effective_attributes = Some(members.into_boxed_slice());
        Some(id)
    }

    pub(super) fn geometry_structure(
        &mut self,
        owner: usize,
        key: &str,
        family: &str,
        template: Option<usize>,
    ) -> Option<usize> {
        let name = Name {
            value: key.into(),
            span: Span::predefined(),
        };
        if let Some(&existing) = self.names.get(&format!(
            "{}.{}",
            self.symbols[owner].declaration.qualified, key
        )) {
            return Some(existing);
        }
        let id = self.add(
            Kind::Structure,
            &name,
            &name.span,
            Some(owner),
            self.symbols[owner].model,
            None,
        )?;
        let origin = if let Some(template) = template {
            Some(self.geometry_symbol_reference(owner, template)?)
        } else {
            None
        };
        let declaration = &mut self.symbols[id].declaration;
        declaration.name = family.into();
        declaration.predefined = template.is_none();
        declaration.derived_from = origin;
        Some(id)
    }

    pub(super) fn geometry_symbol_reference(
        &mut self,
        owner: usize,
        id: usize,
    ) -> Option<TypeReference> {
        let name = Name {
            value: self.symbols[id].declaration.name.clone(),
            span: Span::predefined(),
        };
        self.reference_to_symbol(owner, id, &name)
    }

    pub(super) fn geometry_structure_list(&mut self, owner: usize, id: usize) -> Option<usize> {
        let reference = self.geometry_symbol_reference(owner, id)?;
        let element = self.structure_type_id(Some(reference));
        let ty = self.types.len();
        self.types.push(TypeInfo::Collection(CollectionType {
            kind: CollectionKind::List,
            cardinality: Cardinality {
                lower: "0".into(),
                upper: None,
            },
            element,
        }));
        Some(ty)
    }

    pub(super) fn structure_type_id(&mut self, target: Option<TypeReference>) -> usize {
        let ty = self.types.len();
        self.types.push(TypeInfo::Structure(StructureType {
            target,
            restrictions: Vec::new(),
        }));
        ty
    }

    pub(super) fn contextual_geometry_attribute(
        &mut self,
        owner: usize,
        template: usize,
        name: &str,
        ty: usize,
    ) -> Option<usize> {
        let reference = self
            .visible_name(
                template,
                name,
                Some(&[Kind::Attribute]),
                &Span::predefined(),
            )
            .ok()??;
        let mandatory = self.symbols[reference].declaration.mandatory() == Some(true);
        let origin = self.geometry_symbol_reference(owner, reference)?;
        let id = self.predefined_attribute(owner, name, ty, mandatory, None)?;
        let declaration = &mut self.symbols[id].declaration;
        declaration.predefined = false;
        declaration.derived_from = Some(origin);
        Some(id)
    }

    pub(super) fn line_inspection_type(
        &mut self,
        owner: usize,
        geometry: usize,
        at: &Span,
    ) -> Option<usize> {
        let TypeInfo::Geometry(descriptor) = &self.types[geometry] else {
            unreachable!("line inspections use a geometry");
        };
        if descriptor
            .line_forms
            .iter()
            .any(|form| matches!(form, crate::LineForm::Named(_)))
        {
            self.diagnostics.push(
                "ILI-UNSUPPORTED",
                "Geometry inspection of a custom line form requires contextual segment binding",
                Some(at.clone()),
            );
            return None;
        }
        let vertex = descriptor.vertex.as_ref()?.clone();
        let Some(coordinates) = self.geometry_coordinate(&vertex)? else {
            self.unsupported_expression(at);
            return None;
        };
        let template = self.predefined_geometry_structure(owner, "LineGeometry", at)?;
        let row = self.geometry_structure(
            owner,
            &format!("$LineGeometry{geometry}"),
            "LineGeometry",
            Some(template),
        )?;
        if self.symbols[row].declaration.effective_attributes.is_none() {
            let mut base_segment = None;
            let mut start = None;
            for family in [
                "LineSegment",
                "StartSegment",
                "StraightSegment",
                "ArcSegment",
            ] {
                let template = self.predefined_segment(owner, family, at)?;
                let segment = self.geometry_structure(row, family, family, Some(template))?;
                let mut members = Vec::new();
                if let Some(base) = base_segment {
                    let reference = self.geometry_symbol_reference(owner, base)?;
                    self.symbols[segment].base_scope = Some(base);
                    self.symbols[segment].declaration.base_reference = Some(reference);
                    members.extend_from_slice(
                        self.symbols[base]
                            .declaration
                            .effective_attributes
                            .as_deref()?,
                    );
                }
                self.symbols[segment].declaration.final_declaration =
                    self.symbols[template].declaration.is_final();
                self.symbols[segment].declaration.abstract_declaration =
                    self.symbols[template].declaration.is_abstract();
                let originals = self.symbols[template]
                    .declaration
                    .effective_attributes
                    .as_deref()?
                    .to_vec();
                for original in originals {
                    if self.symbols[original].declaration.parent != Some(template) {
                        continue;
                    }
                    let name = self.symbols[original].declaration.name.clone();
                    let specialize = matches!(name.as_str(), "SegmentEndPoint" | "ArcPoint");
                    let ty = if specialize {
                        coordinates
                    } else {
                        self.symbols[original].declaration.type_id?
                    };
                    let attr = self.contextual_geometry_attribute(segment, template, &name, ty)?;
                    if specialize {
                        self.symbols[attr].declaration.reference = Some(vertex.clone());
                    }
                    members.push(attr);
                }
                self.symbols[segment].declaration.effective_attributes =
                    Some(members.into_boxed_slice());
                if family == "LineSegment" {
                    base_segment = Some(segment);
                } else if family == "StartSegment" {
                    start = Some(segment);
                }
            }
            let segment = base_segment?;
            let list = self.geometry_structure_list(owner, segment)?;
            let member = self.contextual_geometry_attribute(row, template, "Segments", list)?;
            self.symbols[row].declaration.effective_attributes = Some(Box::new([member]));
            self.geometry_start_constraint(owner, row, member, segment, start?)?;
        }
        let reference = self.geometry_symbol_reference(owner, row)?;
        Some(self.structure_type_id(Some(reference)))
    }

    fn geometry_start_constraint(
        &mut self,
        owner: usize,
        structure: usize,
        segments: usize,
        segment: usize,
        start: usize,
    ) -> Option<()> {
        let span = Span::predefined();
        let reference = self.geometry_symbol_reference(owner, segments)?;
        let segment_type = self.geometry_symbol_reference(owner, segment)?;
        let ty = self.structure_type_id(Some(segment_type));
        let first = self.record_expression(
            BoundExpressionKind::Path {
                steps: vec![BoundPathStep {
                    reference,
                    selection: Some(crate::PathSelection::First),
                    selection_span: None,
                    association_access: None,
                }],
            },
            ExpressionType::Object,
            Some(ty),
            &span,
        );
        let reference = self.geometry_symbol_reference(owner, start)?;
        let class_type = self.types.len();
        self.types.push(TypeInfo::ClassValue(crate::ClassValueType {
            kind: crate::ClassValueKind::Structure,
            restrictions: vec![reference.clone()],
        }));
        let class = self.record_expression(
            BoundExpressionKind::Class { reference },
            ExpressionType::Class,
            Some(class_type),
            &span,
        );
        let function = self.predefined_reference(owner, "isOfClass");
        let boolean = self.predefined_type(owner, "BOOLEAN", &span)?.ty;
        let condition = self.record_expression(
            BoundExpressionKind::Call {
                function,
                arguments: vec![first, class],
            },
            ExpressionType::Boolean,
            Some(boolean),
            &span,
        );
        self.symbols[structure].declaration.constraints = Some(Box::new([BoundConstraint {
            name: None,
            target: None,
            rule: ConstraintRule::Mandatory,
            expression: Some(condition),
            span,
            source_metadata: None,
        }]));
        Some(())
    }

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
                                association_access: None,
                            }],
                        },
                        ExpressionType::Text,
                        Some(ty),
                        &name.span,
                    );
                    constraints.push(BoundConstraint {
                        name: None,
                        target: None,
                        expression: None,
                        span: name.span,
                        source_metadata: None,
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

    pub(super) fn predefined_year(&mut self, owner: usize, at: &Span) -> Option<usize> {
        if let Some(&id) = self.names.get("INTERLIS.GregorianYear") {
            return self.symbols[id].declaration.type_id;
        }
        let model = self.symbols[owner].model;
        let namespace = self.predefined_namespace(model)?;
        let name = Name {
            value: "GregorianYear".into(),
            span: Span::predefined(),
        };
        let id = self.add(
            Kind::Domain,
            &name,
            &name.span,
            Some(namespace),
            model,
            None,
        )?;
        let ty = self.predefined_time_range(
            owner,
            ("1582", "2999"),
            "Y",
            false,
            Some("GregorianCalendar"),
            at,
        )?;
        self.symbols[id].declaration.predefined = true;
        self.symbols[id].declaration.type_id = Some(ty);
        self.symbols[id].declaration.effective_mandatory = Some(false);
        Some(ty)
    }

    fn predefined_time_range(
        &mut self,
        owner: usize,
        (lower, upper): (&str, &str),
        unit: &str,
        circular: bool,
        system: Option<&str>,
        at: &Span,
    ) -> Option<usize> {
        let decimal = |value: &str| crate::DecimalInfo {
            original: value.into(),
            coefficient: value.replace('.', ""),
            fractional_digits: value
                .split_once('.')
                .map_or(0, |(_, digits)| digits.len() as u32),
            exponent: 0,
            scaled: false,
            computed: false,
        };
        let lower = decimal(lower);
        let upper = decimal(upper);
        let reference_system = if let Some(name) = system {
            self.predefined_time_basket(owner, at)?;
            Some(NumericReferenceSystem::MetaObject {
                object: self.predefined_reference(owner, &format!("BaseTimeSystems.{name}")),
                axis: None,
            })
        } else {
            None
        };
        let ty = self.types.len();
        self.types.push(TypeInfo::Numeric(NumericRange {
            bounds: Some(NumericBounds { lower, upper }),
            unit: Some(self.predefined_reference(owner, unit)),
            reference_system,
            circular,
            direction: None,
        }));
        Some(ty)
    }

    fn predefined_time_structure(&mut self, owner: usize, name: &str, at: &Span) -> Option<usize> {
        let (base, fields) = match name {
            "GregorianDate" => (None, &["Year", "Month", "Day"][..]),
            "GregorianDateTime" => (Some("GregorianDate"), &["Hours", "Minutes", "Seconds"][..]),
            "TimeOfDay" => (None, &["Hours", "Minutes", "Seconds"][..]),
            "UTC" => (Some("TimeOfDay"), &["Hours"][..]),
            _ => return None,
        };
        if let Some(&id) = self.names.get(&format!("INTERLIS.{name}")) {
            return Some(id);
        }
        let parent = if let Some(base) = base {
            Some((
                self.predefined_time_structure(owner, base, at)?,
                self.predefined_reference(owner, base),
            ))
        } else {
            None
        };
        let namespace = self.predefined_namespace(self.symbols[owner].model)?;
        let id = self.geometry_structure(namespace, name, name, None)?;
        self.symbols[id].declaration.abstract_declaration = name == "TimeOfDay";
        let mut members = Vec::new();
        if let Some((base, reference)) = parent {
            self.symbols[id].base_scope = Some(base);
            self.symbols[id].declaration.base_reference = Some(reference);
            members.extend_from_slice(
                self.symbols[base]
                    .declaration
                    .effective_attributes
                    .as_deref()?,
            );
        }
        for &field in fields {
            let reference =
                (field == "Year").then(|| self.predefined_reference(owner, "GregorianYear"));
            let (ty, subdivision) = match field {
                "Year" => (self.predefined_year(owner, at)?, None),
                "Month" => (
                    self.predefined_time_range(owner, ("1", "12"), "M", false, None, at)?,
                    Some(false),
                ),
                "Day" => (
                    self.predefined_time_range(owner, ("1", "31"), "d", false, None, at)?,
                    Some(false),
                ),
                "Hours" => {
                    let system = (name != "TimeOfDay").then_some("UTC");
                    (
                        self.predefined_time_range(owner, ("0", "23"), "h", true, system, at)?,
                        (name == "GregorianDateTime").then_some(false),
                    )
                }
                "Minutes" => (
                    self.predefined_time_range(owner, ("0", "59"), "min", true, None, at)?,
                    Some(true),
                ),
                "Seconds" => (
                    self.predefined_time_range(owner, ("0.000", "59.999"), "s", true, None, at)?,
                    Some(true),
                ),
                _ => unreachable!(),
            };
            let member = self.predefined_attribute(id, field, ty, false, reference)?;
            self.symbols[member].declaration.subdivision =
                subdivision.map(|continuous| crate::AttributeSubdivision {
                    continuous,
                    span: Span::predefined(),
                });
            if name == "UTC" {
                let base = members[0];
                self.symbols[member].base_scope = Some(base);
                self.symbols[member].declaration.base_reference =
                    Some(self.geometry_symbol_reference(owner, base)?);
                members[0] = member;
            } else {
                members.push(member);
            }
        }
        self.symbols[id].declaration.effective_attributes = Some(members.into_boxed_slice());
        Some(id)
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
                    .or_else(|| self.predefined_geometry_structure(owner, name, at))
                    .or_else(|| self.predefined_time_structure(owner, name, at))
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
