use super::{
    Analyzer, Kind, LookupError, Name, NumberError, QualifiedName, SyntaxDecl,
    domains::scalar_extension, units::unit_extends,
};
use crate::{
    AttributePathOwner, AttributePathType, ClassValueType, CollectionType, CoordinateKind,
    CoordinateType, DecimalInfo, FormattedPart, GeometryKind, GeometryType, LanguageVersion,
    LineForm, NumericRange, NumericReferenceSystem, ObjectSelection, ObjectTarget,
    OverlapTolerance, ReferenceType, Span, StructureType, TypeInfo, TypeReference, syntax,
};
use std::collections::{BTreeMap, BTreeSet};

impl Analyzer<'_, '_> {
    pub(super) fn contextual_line_segments(
        &mut self,
        row: usize,
        forms: &[usize],
        mut symbols: BTreeMap<usize, usize>,
        vertex: &TypeReference,
        coordinates: usize,
        at: &Span,
    ) -> Option<()> {
        let placeholder = self.line_coord_type?;
        let mut pending = Vec::new();
        for form in forms {
            let segment = self.symbols[*form].declaration.type_reference()?;
            pending.push(self.names[segment.target_name()]);
        }
        let mut users: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut visited = BTreeSet::new();
        let mut affected = BTreeSet::new();
        while let Some(structure) = pending.pop() {
            if !visited.insert(structure) || symbols.contains_key(&structure) {
                continue;
            }
            if !self.step(1, at) {
                return None;
            }
            let mut structures = BTreeSet::new();
            structures.extend(self.symbols[structure].base_scope);
            let mut types = self.symbols[structure]
                .declaration
                .effective_attributes
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter_map(|&member| self.symbols[member].declaration.type_id)
                .collect::<Vec<_>>();
            let mut seen = BTreeSet::new();
            while let Some(ty) = types.pop() {
                if !seen.insert(ty) {
                    continue;
                }
                if !self.step(1, at) {
                    return None;
                }
                if ty == placeholder {
                    affected.insert(structure);
                }
                let (links, children) = segment_type_links(&self.types[ty]);
                types.extend(children);
                for link in links {
                    if self.is_line_coord_reference(link, placeholder) {
                        affected.insert(structure);
                    }
                    if let Some(&target) = self.names.get(link.target_name())
                        && self.symbols[target].declaration.kind == Kind::Structure
                    {
                        structures.insert(target);
                    }
                }
            }
            for target in structures {
                users.entry(target).or_default().push(structure);
                if symbols.contains_key(&target) {
                    affected.insert(structure);
                } else {
                    pending.push(target);
                }
            }
        }
        pending.extend(&affected);
        while let Some(target) = pending.pop() {
            for &owner in users.get(&target).into_iter().flatten() {
                if !self.step(1, at) {
                    return None;
                }
                if affected.insert(owner) {
                    pending.push(owner);
                }
            }
        }
        let mut members = Vec::new();
        for &original in &affected {
            let name = self.symbols[original].declaration.name.clone();
            let contextual = self.geometry_structure(
                row,
                &format!("$Segment{original}"),
                &name,
                Some(original),
            )?;
            symbols.insert(original, contextual);
            self.symbols[contextual].model = self.symbols[original].model;
            self.symbols[contextual].ast = self.symbols[original].ast;
            self.symbols[contextual].declaration.final_declaration =
                self.symbols[original].declaration.is_final();
            self.symbols[contextual].declaration.abstract_declaration =
                self.symbols[original].declaration.is_abstract();
            self.symbols[contextual].declaration.source_metadata =
                self.symbols[original].declaration.source_metadata.clone();
            let local = self.symbols[original]
                .declaration
                .effective_attributes
                .as_deref()?
                .to_vec();
            for member in local {
                let declaration = &self.symbols[member].declaration;
                if declaration.parent != Some(original) {
                    continue;
                }
                let (kind, mandatory, model) = (
                    declaration.kind,
                    declaration.declared_mandatory,
                    self.symbols[member].model,
                );
                let name = Name {
                    value: declaration.name.clone(),
                    span: Span::predefined(),
                };
                let copied =
                    self.add(kind, &name, &name.span, Some(contextual), model, mandatory)?;
                symbols.insert(member, copied);
                members.push((member, copied));
            }
        }
        let mut needed = BTreeSet::new();
        let mut pending: Vec<_> = members
            .iter()
            .filter_map(|&(original, _)| self.symbols[original].declaration.type_id)
            .collect();
        while let Some(ty) = pending.pop() {
            if needed.insert(ty) {
                if !self.step(1, at) {
                    return None;
                }
                pending.extend(segment_type_links(&self.types[ty]).1);
            }
        }
        let mut types = BTreeMap::from([(placeholder, coordinates)]);
        for ty in needed {
            if ty == placeholder {
                continue;
            }
            let (links, children) = segment_type_links(&self.types[ty]);
            let changed = children.iter().any(|id| types.contains_key(id))
                || links.iter().any(|link| {
                    self.is_line_coord_reference(link, placeholder)
                        || self
                            .names
                            .get(link.target_name())
                            .is_some_and(|id| symbols.contains_key(id))
                });
            if !self.step(links.len() + children.len(), at) {
                return None;
            }
            if changed {
                let replacement =
                    self.rewrite_segment_type(ty, &types, &symbols, vertex, placeholder, at)?;
                let id = self.types.len();
                self.types.push(replacement);
                types.insert(ty, id);
            }
        }
        for &(original, copied) in &members {
            let from = &self.symbols[original];
            let declaration = &from.declaration;
            let ty = declaration.type_id?;
            let ty = *types.get(&ty).unwrap_or(&ty);
            let reference = declaration
                .reference
                .as_ref()
                .map(|link| self.segment_reference(link, &symbols, vertex, placeholder));
            let base = from.base_scope.map(|id| *symbols.get(&id).unwrap_or(&id));
            let source = self.geometry_symbol_reference(row, original)?;
            let base_reference = if let Some(base) = base {
                Some(self.geometry_symbol_reference(row, base)?)
            } else {
                None
            };
            let declaration = &self.symbols[original].declaration;
            let data = (
                declaration.effective_mandatory,
                declaration.is_final(),
                declaration.is_abstract(),
                declaration.is_transient(),
                declaration.subdivision.clone(),
                declaration.source_metadata.clone(),
            );
            let ast = self.symbols[original].ast;
            let syntax = self.symbols[original].syntax;
            let target = &mut self.symbols[copied];
            target.ast = ast;
            target.syntax = syntax;
            target.base_scope = base;
            target.declaration.type_id = Some(ty);
            target.declaration.reference = reference;
            target.declaration.base_reference = base_reference;
            target.declaration.derived_from = Some(source);
            (
                target.declaration.effective_mandatory,
                target.declaration.final_declaration,
                target.declaration.abstract_declaration,
                target.declaration.transient,
                target.declaration.subdivision,
                target.declaration.source_metadata,
            ) = data;
        }
        for original in affected {
            let copied = symbols[&original];
            let base = self.symbols[original]
                .base_scope
                .map(|id| *symbols.get(&id).unwrap_or(&id));
            self.symbols[copied].base_scope = base;
            if let Some(base) = base {
                self.symbols[copied].declaration.base_reference =
                    Some(self.geometry_symbol_reference(row, base)?);
            }
            let members = self.symbols[original]
                .declaration
                .effective_attributes
                .as_deref()?
                .iter()
                .map(|id| *symbols.get(id).unwrap_or(id))
                .collect::<Box<[_]>>();
            self.symbols[copied].declaration.effective_attributes = Some(members);
        }
        for (original, copied) in members {
            let base = self.symbols[copied].base_scope;
            let ty = self.symbols[copied].declaration.type_id?;
            if let Some(base) = base {
                let base_ty = self.symbols[base].declaration.type_id;
                if let Some(base_ty) = base_ty
                    && matches!(
                        (&self.types[ty], &self.types[base_ty]),
                        (TypeInfo::Coordinate(_), TypeInfo::Coordinate(_))
                    )
                    && !self.coordinate_value_fits(
                        ty,
                        base_ty,
                        self.expression_language(copied),
                        at,
                    )?
                {
                    self.diagnostics.push_related(
                        "ILI-LINE-FORM-VERTEX",
                        "The segment coordinate does not fit the geometry's vertex type",
                        Some(at.clone()),
                        vec![crate::RelatedLocation {
                            span: self.symbols[original].name_span.clone(),
                            message: "Segment attribute".into(),
                        }],
                    );
                    return None;
                }
            }
        }
        Some(())
    }

    fn is_line_coord_reference(&self, reference: &TypeReference, placeholder: usize) -> bool {
        reference.target_name() == "INTERLIS.LineCoord"
            || self.names.get(reference.target_name()).is_some_and(|&id| {
                self.symbols[id].declaration.kind == Kind::Domain
                    && self.symbols[id].declaration.type_id == Some(placeholder)
            })
    }

    fn segment_reference(
        &self,
        reference: &TypeReference,
        symbols: &BTreeMap<usize, usize>,
        vertex: &TypeReference,
        placeholder: usize,
    ) -> TypeReference {
        let mut result = reference.clone();
        if self.is_line_coord_reference(reference, placeholder) {
            result.target = vertex.target.clone();
            result.predefined_language = vertex.predefined_language;
        } else if let Some(&id) = self
            .names
            .get(reference.target_name())
            .and_then(|id| symbols.get(id))
        {
            result.target = self.symbols[id].declaration.qualified.clone();
            result.predefined_language = None;
        }
        result
    }

    fn rewrite_segment_type(
        &mut self,
        ty: usize,
        types: &BTreeMap<usize, usize>,
        symbols: &BTreeMap<usize, usize>,
        vertex: &TypeReference,
        placeholder: usize,
        at: &Span,
    ) -> Option<TypeInfo> {
        let reference =
            |link: &TypeReference| self.segment_reference(link, symbols, vertex, placeholder);
        let id = |ty: usize| *types.get(&ty).unwrap_or(&ty);
        let target = |target: &ObjectTarget| ObjectTarget {
            target: target.target.as_ref().map(reference),
            restrictions: target.restrictions.iter().map(reference).collect(),
        };
        let numeric = |value: &NumericRange| {
            let mut value = value.clone();
            if let Some(NumericReferenceSystem::Domain(system)) = &mut value.reference_system {
                system.domain = reference(&system.domain);
            }
            value
        };
        let result = match &self.types[ty] {
            TypeInfo::Numeric(value) => TypeInfo::Numeric(numeric(value)),
            TypeInfo::Coordinate(value) => TypeInfo::Coordinate(CoordinateType {
                kind: value.kind,
                axes: value.axes.iter().map(numeric).collect(),
                rotation: value.rotation,
                reference_system: value.reference_system.clone(),
            }),
            TypeInfo::Structure(value) => TypeInfo::Structure(StructureType {
                target: value.target.as_ref().map(reference),
                restrictions: value.restrictions.iter().map(reference).collect(),
            }),
            TypeInfo::Collection(value) => TypeInfo::Collection(CollectionType {
                kind: value.kind,
                cardinality: value.cardinality.clone(),
                element: id(value.element),
            }),
            TypeInfo::ClassValue(value) => TypeInfo::ClassValue(ClassValueType {
                kind: value.kind,
                restrictions: value.restrictions.iter().map(reference).collect(),
            }),
            TypeInfo::Reference(value) => TypeInfo::Reference(ReferenceType {
                external: value.external,
                target: target(&value.target),
            }),
            TypeInfo::ObjectSelection(value) => TypeInfo::ObjectSelection(ObjectSelection {
                plural: value.plural,
                targets: value.targets.iter().map(target).collect(),
            }),
            TypeInfo::Geometry(value) => {
                let mut value = value.clone();
                value.vertex = value.vertex.as_ref().map(reference);
                value.base_vertex = value.base_vertex.as_ref().map(reference);
                value.line_attributes = value.line_attributes.as_ref().map(reference);
                if let Some(OverlapTolerance::TrackWidth(link)) = &mut value.tolerance {
                    *link = link.as_ref().map(reference);
                }
                TypeInfo::Geometry(value)
            }
            TypeInfo::AttributePath(value) => TypeInfo::AttributePath(AttributePathType {
                owner: value.owner.as_ref().map(|owner| match owner {
                    AttributePathOwner::Viewable(link) => {
                        AttributePathOwner::Viewable(reference(link))
                    }
                    AttributePathOwner::ClassAttribute(id) => {
                        AttributePathOwner::ClassAttribute(*id)
                    }
                    AttributePathOwner::Argument(link) => {
                        AttributePathOwner::Argument(reference(link))
                    }
                }),
                restrictions: value
                    .restrictions
                    .iter()
                    .map(|rule| crate::model::AttributeRestriction {
                        ty: id(rule.ty),
                        mandatory: rule.mandatory,
                    })
                    .collect(),
            }),
            TypeInfo::Formatted(value) => {
                let mut value = value.clone();
                value.structure = reference(&value.structure);
                value.base = value.base.map(id);
                for part in &mut value.parts {
                    match part {
                        FormattedPart::Literal { .. } => {}
                        FormattedPart::Numeric { attribute, .. } => {
                            *attribute = reference(attribute)
                        }
                        FormattedPart::Structure(part) => {
                            part.attribute = reference(&part.attribute);
                            part.type_id = id(part.type_id);
                        }
                    }
                }
                TypeInfo::Formatted(value)
            }
            _ => {
                self.unsupported_expression(at);
                return None;
            }
        };
        Some(result)
    }

    pub(super) fn bind_line_forms(&mut self) -> Option<()> {
        for owner in 0..self.symbols.len() {
            let Some(SyntaxDecl::LineForm(form)) = self.symbols[owner].ast else {
                continue;
            };
            let model = &self.models[self.symbols[owner].model];
            if model.language() == LanguageVersion::V23 && !model.is_contracted() {
                self.diagnostics.push(
                    "ILI-LINE-FORM-CONTRACTED",
                    "A custom line form requires a CONTRACTED model in INTERLIS 2.3",
                    Some(form.name.span.clone()),
                );
                return None;
            }
            let name = QualifiedName {
                segments: vec![syntax::Name {
                    value: form.segment.value.clone(),
                    span: form.segment.span.clone(),
                }],
                span: form.segment.span.clone(),
            };
            let bound =
                self.expression_name(owner, &name, &[Kind::Structure], "ILI-LINE-FORM-STRUCTURE")?;
            let segment = self.bound_structure(owner, bound, &name.span)?;
            let base = self.predefined_segment(owner, "LineSegment", &form.segment.span)?;
            if segment == base || !self.specializes_symbol(segment, base, &form.segment.span)? {
                self.diagnostics.push(
                    "ILI-LINE-FORM-STRUCTURE",
                    "The segment structure of a line form must extend INTERLIS.LineSegment",
                    Some(form.segment.span.clone()),
                );
                return None;
            }
            self.symbols[owner].declaration.reference =
                Some(self.bound_reference(owner, &name, bound)?);
        }
        Some(())
    }

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
                && !declaration.is_predefined()
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
        if self.vertex_specializes(vertex, base, language)? {
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

    fn vertex_specializes(
        &mut self,
        vertex: &TypeReference,
        base: &TypeReference,
        language: LanguageVersion,
    ) -> Option<bool> {
        let actual = self.geometry_coordinate(vertex)?;
        let expected = self.geometry_coordinate(base)?;
        match (actual, expected) {
            (_, None) => Some(true),
            (None, Some(_)) => Some(false),
            (Some(actual), Some(expected)) => {
                self.coordinate_value_fits(actual, expected, language, &vertex.span)
            }
        }
    }

    pub(super) fn coordinate_value_fits(
        &mut self,
        actual: usize,
        expected: usize,
        language: LanguageVersion,
        at: &Span,
    ) -> Option<bool> {
        let checked = self
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
        match checked {
            Ok(mismatch) => Some(mismatch.is_none()),
            Err(error) => {
                self.number_error(error, at);
                None
            }
        }
    }

    pub(super) fn geometry_value_fits(
        &mut self,
        actual: usize,
        expected: usize,
        at: &Span,
    ) -> Option<bool> {
        let (TypeInfo::Geometry(actual), TypeInfo::Geometry(expected)) =
            (&self.types[actual], &self.types[expected])
        else {
            return Some(false);
        };
        let same_family = actual.geometry_kind == expected.geometry_kind
            || matches!(
                (actual.geometry_kind, expected.geometry_kind),
                (GeometryKind::Area, GeometryKind::Surface)
                    | (GeometryKind::MultiArea, GeometryKind::MultiSurface)
            );
        let line_work = actual
            .line_forms
            .len()
            .saturating_mul(expected.line_forms.len());
        if self
            .work
            .charge(u32::try_from(line_work.saturating_add(1)).unwrap_or(u32::MAX))
            .is_err()
        {
            self.diagnostics
                .limit("Semantic work budget exceeded", Some(at.clone()));
            return None;
        }
        let language = expected.language;
        let valid = same_family
            && (!expected.directed || actual.directed)
            && (!expected.without_overlaps || actual.without_overlaps)
            && (expected.line_forms.is_empty()
                || !actual.line_forms.is_empty()
                    && actual.line_forms.iter().all(|actual| {
                        expected
                            .line_forms
                            .iter()
                            .any(|expected| form_name(actual) == form_name(expected))
                    }));
        let line_attributes = (
            actual.line_attributes.clone(),
            expected.line_attributes.clone(),
        );
        let vertices = (actual.vertex.clone(), expected.vertex.clone());
        let tolerances = (actual.tolerance.clone(), expected.tolerance.clone());
        if !valid
            || (vertices.0.is_none() && vertices.1.is_some())
            || (line_attributes.0.is_none() && line_attributes.1.is_some())
        {
            return Some(false);
        }
        if let (Some(actual), Some(expected)) = vertices
            && !self.vertex_specializes(&actual, &expected, language)?
        {
            return Some(false);
        }
        if let Some(expected) = &tolerances.1
            && !self.tolerance_fits(tolerances.0.as_ref(), expected, at)?
        {
            return Some(false);
        }
        if let (Some(actual), Some(expected)) = line_attributes {
            return self.specializes_symbol(
                self.names[actual.target_name()],
                self.names[expected.target_name()],
                at,
            );
        }
        Some(true)
    }

    fn tolerance_fits(
        &mut self,
        actual: Option<&OverlapTolerance>,
        expected: &OverlapTolerance,
        at: &Span,
    ) -> Option<bool> {
        use num_bigint::BigUint;
        let Some((mut expected, expected_scale)) = self.squared_tolerance(expected, at)? else {
            // An abstract coordinate domain does not specify a track width.
            return Some(true);
        };
        let Some(actual) = actual else {
            return Some(false);
        };
        let Some((mut actual, actual_scale)) = self.squared_tolerance(actual, at)? else {
            return Some(false);
        };
        let difference = actual_scale.abs_diff(expected_scale);
        if difference != 0 {
            if !self.step(difference as usize, at) {
                return None;
            }
            let scale = BigUint::from(10_u8).pow(difference);
            if actual_scale < expected_scale {
                actual *= scale;
            } else {
                expected *= scale;
            }
        }
        Some(actual <= expected)
    }

    fn squared_tolerance(
        &mut self,
        tolerance: &OverlapTolerance,
        at: &Span,
    ) -> Option<Option<(num_bigint::BigUint, i32)>> {
        use num_bigint::BigUint;
        // Compare 2*t². A track width gives q², with q the coordinate quantum.
        match tolerance {
            OverlapTolerance::Explicit(value) => {
                let size = value.coefficient().len();
                if !self.step(size.saturating_mul(size), at) {
                    return None;
                }
                let coefficient = BigUint::parse_bytes(value.coefficient().as_bytes(), 10)?;
                Some(Some((
                    (&coefficient * &coefficient) * 2_u8,
                    2 * value.scale(),
                )))
            }
            OverlapTolerance::TrackWidth(reference) => {
                let coordinates = if let Some(reference) = reference {
                    self.geometry_coordinate(reference)?
                } else {
                    None
                };
                Some(coordinates.and_then(|id| {
                    match &self.types[id] {
                        TypeInfo::Coordinate(coordinate) => coordinate
                            .axes
                            .first()?
                            .bounds
                            .as_ref()
                            .map(|bounds| (BigUint::from(1_u8), 2 * bounds.lower.scale())),
                        _ => None,
                    }
                }))
            }
        }
    }

    pub(super) fn geometry_coordinate(&mut self, vertex: &TypeReference) -> Option<Option<usize>> {
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

fn segment_type_links(ty: &TypeInfo) -> (Vec<&TypeReference>, Vec<usize>) {
    let mut links = Vec::new();
    let mut children = Vec::new();
    fn reference_system(value: &NumericRange) -> Option<&TypeReference> {
        match &value.reference_system {
            Some(NumericReferenceSystem::Domain(system)) => Some(&system.domain),
            _ => None,
        }
    }
    match ty {
        TypeInfo::Numeric(value) => links.extend(reference_system(value)),
        TypeInfo::Coordinate(value) => links.extend(value.axes.iter().filter_map(reference_system)),
        TypeInfo::Structure(value) => {
            links.extend(value.target.iter());
            links.extend(value.restrictions.iter());
        }
        TypeInfo::ClassValue(value) => links.extend(value.restrictions.iter()),
        TypeInfo::Reference(value) => {
            links.extend(value.target.target.iter());
            links.extend(value.target.restrictions.iter());
        }
        TypeInfo::ObjectSelection(value) => {
            for target in &value.targets {
                links.extend(target.target.iter());
                links.extend(target.restrictions.iter());
            }
        }
        TypeInfo::Collection(value) => children.push(value.element),
        TypeInfo::Geometry(value) => {
            links.extend(value.vertex.iter());
            links.extend(value.base_vertex.iter());
            links.extend(value.line_attributes.iter());
        }
        TypeInfo::AttributePath(value) => {
            if let Some(AttributePathOwner::Viewable(link)) = &value.owner {
                links.push(link);
            }
            children.extend(value.restrictions.iter().map(|rule| rule.ty));
        }
        TypeInfo::Formatted(value) => {
            links.push(&value.structure);
            children.extend(value.base);
            for part in &value.parts {
                match part {
                    FormattedPart::Literal { .. } => {}
                    FormattedPart::Numeric { attribute, .. } => links.push(attribute),
                    FormattedPart::Structure(part) => {
                        links.push(&part.attribute);
                        children.push(part.type_id);
                    }
                }
            }
        }
        _ => {}
    }
    (links, children)
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
