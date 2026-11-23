use super::{
    Analyzer, Kind, LookupError, NumberError, QualifiedName, domains::scalar_extension,
    units::unit_extends,
};
use crate::{
    CoordinateKind, DecimalInfo, GeometryKind, GeometryType, LanguageVersion, LineForm,
    OverlapTolerance, Span, TypeInfo, TypeReference, syntax,
};
use std::collections::BTreeSet;

impl Analyzer<'_, '_> {
    pub(super) fn geometry_type(
        &mut self,
        owner: usize,
        syntax: &syntax::GeometrySyntax,
    ) -> Option<GeometryType> {
        let language = self.models[self.symbols[owner].model].language;
        let mut line_forms = Vec::with_capacity(syntax.line_forms.len());
        let mut seen = BTreeSet::new();
        for form in &syntax.line_forms {
            let (value, span) = match form {
                syntax::LineFormSyntax::Straights(span) => (LineForm::Straights, span),
                syntax::LineFormSyntax::Arcs(span) => (LineForm::Arcs, span),
                syntax::LineFormSyntax::Named(name) => (
                    LineForm::Named(self.geometry_reference(owner, name, Kind::LineForm)?),
                    &name.span,
                ),
            };
            if !seen.insert(form_name(&value).to_owned()) {
                self.diagnostics.push(
                    "ILI-LINE-FORM-DUPLICATE",
                    "A line form must be listed only once",
                    Some(span.clone()),
                );
                return None;
            }
            line_forms.push(value);
        }
        let vertex = match &syntax.vertex {
            Some(name) => Some(self.geometry_reference(owner, name, Kind::Domain)?),
            None => None,
        };
        let surface = is_surface(syntax.kind);
        let line_attributes = if let Some(name) = &syntax.line_attributes {
            if !surface {
                self.diagnostics.push(
                    "ILI-GEOMETRY-LINE-ATTRIBUTES",
                    "LINE ATTRIBUTES requires a surface or area",
                    Some(name.span.clone()),
                );
                return None;
            }
            Some(self.structure_reference(owner, name)?)
        } else {
            None
        };
        let without_overlaps = surface || syntax.overlaps.is_some();
        let tolerance = if let Some(value) = syntax
            .overlaps
            .as_ref()
            .and_then(|overlap| overlap.tolerance.as_ref())
        {
            let value_info = self.literal(value)?.info;
            if value_info.coefficient() == "0" || value_info.coefficient().starts_with('-') {
                self.diagnostics.push(
                    "ILI-GEOMETRY-TOLERANCE",
                    "An overlap tolerance must be greater than zero",
                    Some(value.span.clone()),
                );
                return None;
            }
            Some(OverlapTolerance::Explicit(value_info))
        } else if language == LanguageVersion::V24 && without_overlaps {
            Some(OverlapTolerance::TrackWidth(vertex.clone()))
        } else {
            None
        };
        Some(GeometryType {
            geometry_kind: syntax.kind,
            directed: syntax.directed.is_some(),
            line_forms,
            vertex,
            without_overlaps,
            tolerance,
            line_attributes,
            language,
            span: syntax.span.clone(),
            base_vertex: None,
            explicit_tolerance: syntax
                .overlaps
                .as_ref()
                .is_some_and(|overlap| overlap.tolerance.is_some()),
        })
    }

    fn geometry_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kind: Kind,
    ) -> Option<TypeReference> {
        match self.lookup_name(owner, &name.segments, &name.span, &[kind]) {
            Ok(target) => self.bound_reference(owner, name, target),
            Err(LookupError::Limit) => None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "Geometry reference cannot be resolved",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    pub(super) fn specialize_geometry(
        &mut self,
        owner: usize,
        own_id: usize,
        base_id: usize,
        inline: bool,
        span: &Span,
    ) -> Option<()> {
        if own_id == base_id {
            return Some(());
        }
        let (TypeInfo::Geometry(own), TypeInfo::Geometry(base)) =
            (&self.types[own_id], &self.types[base_id])
        else {
            self.report_type_override(
                owner,
                "ILI-GEOMETRY-EXTENSION",
                "A geometry requires a geometric base",
                span,
            );
            return None;
        };
        let family = own.geometry_kind == base.geometry_kind
            || matches!(
                (own.geometry_kind, base.geometry_kind),
                (GeometryKind::Area, GeometryKind::Surface)
                    | (GeometryKind::MultiArea, GeometryKind::MultiSurface)
            );
        if !family
            || base.directed && !own.directed
            || !base.line_forms.is_empty() && own.line_forms.is_empty() && !inline
            || !base.line_forms.is_empty()
                && !own.line_forms.iter().all(|actual| {
                    base.line_forms
                        .iter()
                        .any(|expected| form_name(actual) == form_name(expected))
                })
        {
            self.report_type_override(owner, "ILI-GEOMETRY-EXTENSION", "A geometry extension must preserve its family and direction and narrow its line forms", span);
            return None;
        }
        let inherits_tolerance = inline && !own.explicit_tolerance;
        if let Some(inherited) = &base.tolerance
            && !inherits_tolerance
            && !own
                .tolerance
                .as_ref()
                .is_some_and(|actual| same_tolerance(actual, inherited))
        {
            self.report_type_override(
                owner,
                "ILI-GEOMETRY-TOLERANCE-EXTENSION",
                "An inherited overlap tolerance cannot change",
                span,
            );
            return None;
        }
        if let (Some(actual), Some(expected)) = (&own.line_attributes, &base.line_attributes) {
            let actual = self.names[actual.target_name()];
            let expected = self.names[expected.target_name()];
            if !self.specializes_symbol(actual, expected, span)? {
                self.report_type_override(
                    owner,
                    "ILI-GEOMETRY-LINE-ATTRIBUTES",
                    "Line attributes must specialize the inherited structure",
                    span,
                );
                return None;
            }
        }
        if !inline {
            let (TypeInfo::Geometry(own), TypeInfo::Geometry(base)) =
                (&self.types[own_id], &self.types[base_id])
            else {
                unreachable!("geometry pair checked above");
            };
            if base.vertex.is_some() && own.vertex.is_none()
                || base.without_overlaps && !own.without_overlaps
            {
                self.report_type_override(
                    owner,
                    "ILI-GEOMETRY-EXTENSION",
                    "A named geometry override must preserve its inherited vertex and overlap rule",
                    span,
                );
                return None;
            }
            let pair = own.vertex.clone().zip(base.vertex.clone());
            let language = self.models[self.symbols[owner].model].language;
            if let Some((vertex, base)) = pair {
                self.check_vertex_extension(&vertex, &base, language)?;
            }
            return Some(());
        }
        let TypeInfo::Geometry(base) = &self.types[base_id] else {
            unreachable!()
        };
        let work = base
            .line_forms
            .iter()
            .map(|form| form_name(form).len() + 1)
            .sum::<usize>()
            + base.vertex.as_ref().map_or(0, reference_bytes)
            + base.tolerance.as_ref().map_or(0, tolerance_bytes);
        if !self.step(work, span) {
            return None;
        }
        let [TypeInfo::Geometry(own), TypeInfo::Geometry(base)] =
            self.types.get_disjoint_mut([own_id, base_id]).unwrap()
        else {
            unreachable!("geometry pair checked above");
        };
        if own.line_forms.is_empty() {
            own.line_forms.clone_from(&base.line_forms);
        }
        if own.vertex.is_none() {
            own.vertex.clone_from(&base.vertex);
        } else {
            own.base_vertex.clone_from(&base.vertex);
        }
        own.without_overlaps |= base.without_overlaps;
        if inherits_tolerance && base.tolerance.is_some() {
            own.tolerance.clone_from(&base.tolerance);
        }
        if own.without_overlaps
            && own.language == LanguageVersion::V24
            && matches!(
                own.tolerance,
                None | Some(OverlapTolerance::TrackWidth(None))
            )
        {
            own.tolerance = Some(OverlapTolerance::TrackWidth(own.vertex.clone()));
        }
        Some(())
    }

    pub(super) fn validate_geometries(&mut self) -> Option<()> {
        let mut incomplete = BTreeSet::new();
        for id in 0..self.types.len() {
            let TypeInfo::Geometry(geometry) = &self.types[id] else {
                continue;
            };
            let language = geometry.language;
            let span = geometry.span.clone();
            let mut abstract_geometry = geometry.line_forms.is_empty()
                || geometry.vertex.is_none()
                || geometry.without_overlaps && geometry.tolerance.is_none();
            let vertex = geometry.vertex.clone();
            let base_vertex = geometry.base_vertex.clone();
            if let Some(vertex) = vertex {
                let coordinates = self.geometry_coordinate(&vertex)?;
                if let Some(coordinates) = coordinates {
                    let TypeInfo::Coordinate(coord) = &self.types[coordinates] else {
                        unreachable!()
                    };
                    abstract_geometry |= coord.axes.iter().any(|axis| axis.bounds.is_none())
                        && !self.generic_types.contains_key(&coordinates)
                        || self
                            .names
                            .get(vertex.target_name())
                            .is_some_and(|&id| self.symbols[id].declaration.is_abstract());
                    if language == LanguageVersion::V24 {
                        self.check_line_precision(id, coordinates, &span)?;
                    }
                    if let Some(base) = base_vertex {
                        self.check_vertex_extension(&vertex, &base, language)?;
                    }
                } else {
                    abstract_geometry = true;
                }
            }
            if abstract_geometry {
                incomplete.insert(id);
            }
        }
        for symbol in &self.symbols {
            let declaration = &symbol.declaration;
            let Some(ty) = declaration.type_id else {
                continue;
            };
            let actual = match &self.types[ty] {
                TypeInfo::Collection(collection) => collection.element,
                _ => ty,
            };
            if declaration.kind == Kind::RuntimeParameter
                && matches!(&self.types[actual], TypeInfo::Geometry(geometry) if matches!(geometry.geometry_kind, GeometryKind::Area | GeometryKind::MultiArea))
            {
                self.diagnostics.push(
                    "ILI-PARAMETER-AREA",
                    "A runtime parameter cannot have an AREA type",
                    Some(symbol.name_span.clone()),
                );
                return None;
            }
            if matches!(declaration.kind, Kind::Domain | Kind::Attribute)
                && !declaration.is_abstract()
                && incomplete.contains(&actual)
            {
                self.diagnostics.push(
                    "ILI-GEOMETRY-ABSTRACT",
                    "A concrete geometry requires line forms, concrete vertices and any required overlap tolerance",
                    Some(symbol.name_span.clone()),
                );
                return None;
            }
        }
        Some(())
    }

    fn check_vertex_extension(
        &mut self,
        vertex: &TypeReference,
        base: &TypeReference,
        language: LanguageVersion,
    ) -> Option<()> {
        let actual = self.geometry_coordinate(vertex)?;
        let expected = self.geometry_coordinate(base)?;
        let narrower = match (actual, expected) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(actual), Some(expected)) => {
                let check = self
                    .check_unit_specialization(actual, expected, false)
                    .and_then(|unit| {
                        if unit.is_some() {
                            Ok(unit)
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
                match check {
                    Ok(result) => result.is_none(),
                    Err(error) => {
                        self.number_error(error, &vertex.span);
                        return None;
                    }
                }
            }
        };
        if narrower {
            Some(())
        } else {
            self.diagnostics.push(
                "ILI-GEOMETRY-VERTEX-EXTENSION",
                "The vertex domain must restrict the inherited coordinates",
                Some(vertex.span.clone()),
            );
            None
        }
    }

    fn geometry_coordinate(&mut self, vertex: &TypeReference) -> Option<Option<usize>> {
        if vertex.target_name() == "INTERLIS.LineCoord" {
            return Some(None);
        }
        if let Some(ty) = self
            .names
            .get(vertex.target_name())
            .and_then(|&id| self.symbols[id].declaration.type_id)
            && matches!(&self.types[ty], TypeInfo::Coordinate(coordinate) if coordinate.kind == CoordinateKind::Single)
        {
            return Some(Some(ty));
        }
        self.diagnostics.push(
            "ILI-GEOMETRY-VERTEX",
            "VERTEX must bind a single-coordinate domain",
            Some(vertex.span.clone()),
        );
        None
    }

    pub(super) fn check_line_precision(
        &mut self,
        geometry: usize,
        coordinates: usize,
        span: &Span,
    ) -> Option<()> {
        let (TypeInfo::Geometry(geometry), TypeInfo::Coordinate(coordinates)) =
            (&self.types[geometry], &self.types[coordinates])
        else {
            unreachable!("checked geometry and coordinate types");
        };
        let length = TypeReference {
            declared: "LENGTH".into(),
            target: "INTERLIS.LENGTH".into(),
            span: span.clone(),
            predefined_language: Some(LanguageVersion::V24),
        };
        let mut precision = None;
        for axis in &coordinates.axes {
            if let Some(unit) = &axis.unit {
                match unit_extends(&self.symbols, &self.names, unit, &length, &mut self.work) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.diagnostics.push(
                            "ILI-GEOMETRY-VERTEX-UNIT",
                            "Line vertices require Cartesian axes with compatible length units",
                            Some(span.clone()),
                        );
                        return None;
                    }
                    Err(NumberError::Work) => {
                        self.diagnostics
                            .limit("Semantic work budget exceeded", Some(span.clone()));
                        return None;
                    }
                    Err(_) => unreachable!("unit traversal only charges semantic work"),
                }
            }
            if let Some(bounds) = &axis.bounds {
                let places = bounds.lower.scale();
                if precision.is_some_and(|expected| places != expected)
                    || bounds.upper.scale() != places
                {
                    self.diagnostics.push(
                        "ILI-GEOMETRY-VERTEX-PRECISION",
                        "Line vertex axes must have the same numerical precision",
                        Some(span.clone()),
                    );
                    return None;
                }
                precision = Some(places);
            }
        }
        if let (Some(precision), Some(OverlapTolerance::Explicit(tolerance))) =
            (precision, &geometry.tolerance)
            && geometry.explicit_tolerance
            && precision != tolerance.scale()
        {
            self.diagnostics.push(
                "ILI-GEOMETRY-TOLERANCE-PRECISION",
                "The overlap tolerance must have the same precision as the vertex coordinates",
                Some(span.clone()),
            );
            return None;
        }
        Some(())
    }
}

fn form_name(form: &LineForm) -> &str {
    match form {
        LineForm::Straights => "STRAIGHTS",
        LineForm::Arcs => "ARCS",
        LineForm::Named(reference) => reference.target_name(),
    }
}

fn is_surface(kind: GeometryKind) -> bool {
    matches!(
        kind,
        GeometryKind::Area
            | GeometryKind::Surface
            | GeometryKind::MultiArea
            | GeometryKind::MultiSurface
    )
}

fn reference_bytes(reference: &TypeReference) -> usize {
    reference.declared.len() + reference.target.len()
}

fn tolerance_bytes(tolerance: &OverlapTolerance) -> usize {
    match tolerance {
        OverlapTolerance::Explicit(value) => value.original().len() + value.coefficient().len(),
        OverlapTolerance::TrackWidth(reference) => reference.as_ref().map_or(0, reference_bytes),
    }
}

fn same_tolerance(actual: &OverlapTolerance, base: &OverlapTolerance) -> bool {
    fn normalized(number: &DecimalInfo) -> (&str, i32) {
        let coefficient = number.coefficient();
        let significant = coefficient.trim_end_matches('0');
        (
            significant,
            number.scale() - (coefficient.len() - significant.len()) as i32,
        )
    }
    match (actual, base) {
        (OverlapTolerance::TrackWidth(_), OverlapTolerance::TrackWidth(_)) => true,
        (OverlapTolerance::Explicit(actual), OverlapTolerance::Explicit(base)) => {
            normalized(actual) == normalized(base)
        }
        _ => false,
    }
}
