use super::{Analyzer, BoundName, Kind, Name};
use crate::{NumericRange, Span, TypeInfo, TypeReference};

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
        let name = Name {
            value: name.into(),
            span: Span::predefined(),
        };
        let id = self.add(
            Kind::Attribute,
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

    fn predefined_reference(&self, owner: usize, name: &str) -> TypeReference {
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
                if let Some(id) = self.predefined_segment(owner, name, at) {
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
