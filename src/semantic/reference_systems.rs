use super::{
    Analyzer, BoundName, Kind, LookupError, NumberError, Symbol,
    collections::{compare_count, structure_targets},
    units::{unit_extends, units_convertible},
};
use crate::{
    DomainReferenceSystem, LanguageVersion, NumericRange, NumericReferenceSystem, OidType,
    RelatedLocation, Span, TypeInfo, TypeReference, limits::WorkBudget, syntax,
};
use std::collections::BTreeMap;

impl Analyzer<'_, '_> {
    pub(super) fn numeric_reference_system(
        &mut self,
        owner: usize,
        syntax: Option<&syntax::ReferenceSystemSyntax>,
    ) -> Option<Option<NumericReferenceSystem>> {
        let (reference, axis, domain) = match syntax {
            Some(syntax::ReferenceSystemSyntax::Domain {
                reference, axis, ..
            }) => (reference, axis, true),
            Some(syntax::ReferenceSystemSyntax::Metaobject {
                reference, axis, ..
            }) => (reference, axis, false),
            None => return Some(None),
        };
        let axis = if let Some(axis) = axis {
            let Some(index) = axis
                .value
                .parse::<u8>()
                .ok()
                .filter(|&value| (1..=3).contains(&value))
            else {
                self.diagnostics.push(
                    "ILI-REFSYSTEM-AXIS",
                    "A reference-system axis must be an existing coordinate axis",
                    Some(axis.span.clone()),
                );
                return None;
            };
            Some(index)
        } else {
            None
        };
        if !domain {
            return Some(Some(NumericReferenceSystem::MetaObject {
                object: self.metaobject_reference(owner, reference)?,
                axis,
            }));
        }
        let target =
            match self.lookup_name(owner, &reference.segments, &reference.span, &[Kind::Domain]) {
                Ok(target) => target,
                Err(LookupError::Limit) => return None,
                Err(_) => {
                    self.diagnostics.push(
                        "ILI-NAME-NOT-FOUND",
                        "The reference-system domain cannot be resolved",
                        Some(reference.span.clone()),
                    );
                    return None;
                }
            };
        if matches!(target, BoundName::Predefined("GregorianYear", Kind::Domain)) {
            self.predefined_year(owner, &reference.span)?;
        }
        Some(Some(NumericReferenceSystem::Domain(
            DomainReferenceSystem {
                domain: self.bound_reference(owner, reference, target)?,
                axis,
            },
        )))
    }

    pub(super) fn validate_domain_reference_systems(&mut self) -> Option<()> {
        let mut edges = BTreeMap::new();
        for id in 0..self.types.len() {
            let ranges = reference_ranges(&self.types[id]);
            for index in 0..ranges.len() {
                let range = &reference_ranges(&self.types[id])[index];
                let Some(reference) = &range.reference_system else {
                    continue;
                };
                if !self.step(1, &reference.target().span.clone()) {
                    return None;
                }
                let range = &reference_ranges(&self.types[id])[index];
                let reference = match range.reference_system.as_ref().unwrap() {
                    NumericReferenceSystem::Domain(reference) => reference,
                    NumericReferenceSystem::MetaObject { object, axis } => {
                        let class = self.names[self.symbols[self.names[object.target_name()]]
                            .declaration
                            .type_reference()?
                            .target_name()];
                        let at = object.source_span().clone();
                        let axis = *axis;
                        let given = range.unit.clone();
                        self.validate_metadata_reference(class, axis, given.as_ref(), &at)?;
                        continue;
                    }
                };
                let at = reference.domain.span.clone();
                let source = (id, index);
                let target = self
                    .names
                    .get(reference.domain.target_name())
                    .and_then(|&id| self.symbols[id].declaration.type_id);
                let mut direction = false;
                let expected = match (target.map(|id| (id, &self.types[id])), reference.axis) {
                    (Some((target, TypeInfo::Numeric(numeric))), None) => {
                        Some((target, 0, numeric))
                    }
                    (Some((target, TypeInfo::Coordinate(coordinate))), Some(axis)) => {
                        let index = usize::from(axis - 1);
                        coordinate
                            .axes
                            .get(index)
                            .map(|range| (target, index, range))
                    }
                    (Some((target, TypeInfo::Coordinate(coordinate))), None)
                        if coordinate.rotation.is_some()
                            || self.generic_types.contains_key(&target) =>
                    {
                        direction = true;
                        None
                    }
                    (None, Some(1 | 2))
                        if reference.domain.target_name() == "INTERLIS.LineCoord" =>
                    {
                        // The predefined axes have no reference or fixed unit.
                        continue;
                    }
                    _ => None,
                };
                if direction {
                    if let Some(unit) = &range.unit {
                        let angle = TypeReference {
                            declared: "ANGLE".into(),
                            target: "INTERLIS.ANGLE".into(),
                            span: at.clone(),
                            predefined_language: reference.domain.predefined_language(),
                        };
                        match unit_extends(&self.symbols, &self.names, unit, &angle, &mut self.work)
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                self.diagnostics.push(
                                    "ILI-REFSYSTEM-DIRECTION-UNIT",
                                    "A coordinate direction uses an angular unit",
                                    Some(at),
                                );
                                return None;
                            }
                            Err(error) => {
                                self.number_error(error, &at);
                                return None;
                            }
                        }
                    }
                    continue;
                }
                let Some((target, axis, range_target)) = expected else {
                    self.diagnostics.push(
                        "ILI-REFSYSTEM-DOMAIN",
                        "A reference system must name a numeric domain, an existing coordinate axis or a rotated coordinate domain",
                        Some(at),
                    );
                    return None;
                };
                if let Some(required) = &range_target.unit {
                    let compatible = compatible_reference_unit(
                        &self.symbols,
                        &self.names,
                        range.unit.as_ref(),
                        required,
                        &mut self.work,
                    );
                    match compatible {
                        Ok(true) => {}
                        Ok(false) => {
                            self.diagnostics.push("ILI-REFSYSTEM-UNIT", "A numeric value must use a unit compatible with its reference system", Some(at));
                            return None;
                        }
                        Err(error) => {
                            self.number_error(error, &at);
                            return None;
                        }
                    }
                }
                edges.insert(source, ((target, axis), at));
            }
        }
        self.check_reference_system_cycles(edges)
    }

    pub(super) fn validate_context_use(
        &mut self,
        (use_type, selected_axis): (usize, Option<usize>),
        generic: usize,
        concrete: usize,
        span: &Span,
    ) -> Option<()> {
        if !self.step(1, span) {
            return None;
        }
        let concrete_type = self.symbols[concrete].declaration.type_id?;
        let TypeInfo::Coordinate(coordinates) = &self.types[concrete_type] else {
            unreachable!("context alternatives are concrete coordinates");
        };
        if let TypeInfo::Geometry(geometry) = &self.types[use_type] {
            if geometry.language == LanguageVersion::V24 {
                self.check_line_precision(use_type, concrete_type, span)?;
            }
            return Some(());
        }
        for (index, range) in reference_ranges(&self.types[use_type]).iter().enumerate() {
            if selected_axis.is_some_and(|axis| axis != index) {
                continue;
            }
            let Some(NumericReferenceSystem::Domain(reference)) = &range.reference_system else {
                continue;
            };
            if self.names.get(reference.domain.target_name()) != Some(&generic) {
                continue;
            }
            let result = if let Some(axis) = reference.axis {
                match coordinates.axes[usize::from(axis - 1)].unit.as_ref() {
                    None => Ok(true),
                    Some(required) => compatible_reference_unit(
                        &self.symbols,
                        &self.names,
                        range.unit.as_ref(),
                        required,
                        &mut self.work,
                    ),
                }
            } else {
                Ok(coordinates.rotation.is_some())
            };
            match result {
                Ok(true) => {}
                Ok(false) => {
                    self.diagnostics.push_related(
                        "ILI-REFSYSTEM-CONTEXT",
                        "The selected coordinate alternative cannot serve as this numeric reference system",
                        Some(span.clone()),
                        vec![
                            RelatedLocation { span: reference.domain.span.clone(), message: "Generic reference system".into() },
                            RelatedLocation { span: self.symbols[concrete].name_span.clone(), message: "Coordinate alternative".into() },
                        ],
                    );
                    return None;
                }
                Err(error) => {
                    self.number_error(error, span);
                    return None;
                }
            }
        }
        Some(())
    }

    fn validate_metadata_reference(
        &mut self,
        class: usize,
        axis: Option<u8>,
        given: Option<&TypeReference>,
        at: &Span,
    ) -> Option<()> {
        if !self.specializes_metadata_family(class, "INTERLIS.REFSYSTEM", at)? {
            self.diagnostics.push(
                "ILI-REFSYSTEM-METAOBJECT",
                "The numeric reference must name a reference-system metaobject",
                Some(at.clone()),
            );
            return None;
        }
        let owners = if let Some(axis) = axis {
            if !self.specializes_metadata_family(class, "INTERLIS.COORDSYSTEM", at)? {
                self.diagnostics.push(
                    "ILI-REFSYSTEM-AXIS",
                    "An axis reference requires a coordinate-system metaobject",
                    Some(at.clone()),
                );
                return None;
            }
            let attribute = self
                .visible_name(class, "Axis", Some(&[Kind::Attribute]), at)
                .ok()??;
            let TypeInfo::Collection(axes) =
                &self.types[self.symbols[attribute].declaration.type_id?]
            else {
                unreachable!("the predefined Axis attribute has a collection type");
            };
            if axes.cardinality.upper.as_ref().is_some_and(|upper| {
                compare_count(&axis.to_string(), upper) == std::cmp::Ordering::Greater
            }) {
                self.diagnostics.push(
                    "ILI-REFSYSTEM-AXIS",
                    "The referenced axis exceeds the coordinate-system axis cardinality",
                    Some(at.clone()),
                );
                return None;
            }
            let TypeInfo::Structure(element) = &self.types[axes.element] else {
                unreachable!("coordinate axes are structures");
            };
            structure_targets(element)
                .iter()
                .map(|target| self.names[target.target_name()])
                .collect::<Vec<_>>()
        } else {
            vec![class]
        };
        for owner in owners {
            let Some(parameter) = self
                .visible_name(owner, "Unit", Some(&[Kind::Parameter]), at)
                .ok()?
            else {
                continue;
            };
            let TypeInfo::Numeric(ty) = &self.types[self.symbols[parameter].declaration.type_id?]
            else {
                unreachable!("metadata unit parameters are numeric");
            };
            if let Some(required) = ty.unit.as_ref() {
                match compatible_reference_unit(
                    &self.symbols,
                    &self.names,
                    given,
                    required,
                    &mut self.work,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.diagnostics.push(
                            "ILI-REFSYSTEM-UNIT",
                            "The numeric unit must be compatible with the selected metadata parameter",
                            Some(at.clone()),
                        );
                        return None;
                    }
                    Err(error) => {
                        self.number_error(error, at);
                        return None;
                    }
                }
            }
        }
        Some(())
    }

    fn check_reference_system_cycles(
        &mut self,
        edges: BTreeMap<(usize, usize), ((usize, usize), Span)>,
    ) -> Option<()> {
        let mut visited = BTreeMap::new();
        for &start in edges.keys() {
            let mut current = start;
            while let Some((target, location)) = edges.get(&current) {
                if visited.contains_key(&current) {
                    break;
                }
                if !self.step(1, location) {
                    return None;
                }
                visited.insert(current, start);
                if visited.get(target) == Some(&start) {
                    self.diagnostics.push(
                        "ILI-REFSYSTEM-CYCLE",
                        "Reference-system domains form a cycle",
                        Some(location.clone()),
                    );
                    return None;
                }
                current = *target;
            }
        }
        Some(())
    }
}

pub(super) fn reference_ranges(ty: &TypeInfo) -> &[NumericRange] {
    match ty {
        TypeInfo::Numeric(range) => std::slice::from_ref(range),
        TypeInfo::Coordinate(coordinate) => &coordinate.axes,
        TypeInfo::Oid(OidType::Typed(ty)) => reference_ranges(ty),
        _ => &[],
    }
}

fn compatible_reference_unit(
    symbols: &[Symbol<'_>],
    names: &BTreeMap<String, usize>,
    actual: Option<&TypeReference>,
    required: &TypeReference,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    let Some(actual) = actual else {
        return Ok(required.target_name() == "INTERLIS.ANYUNIT");
    };
    if unit_extends(symbols, names, actual, required, work)? {
        Ok(true)
    } else {
        units_convertible(symbols, names, actual, required, work)
    }
}
