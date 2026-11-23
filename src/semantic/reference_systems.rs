use super::{
    Analyzer, Kind, LookupError,
    units::{unit_extends, units_convertible},
};
use crate::{DomainReferenceSystem, NumericRange, OidType, Span, TypeInfo, TypeReference, syntax};
use std::collections::BTreeMap;

impl Analyzer<'_, '_> {
    pub(super) fn numeric_reference_system(
        &mut self,
        owner: usize,
        syntax: Option<&syntax::ReferenceSystemSyntax>,
    ) -> Option<Option<DomainReferenceSystem>> {
        let Some(syntax::ReferenceSystemSyntax::Domain {
            reference, axis, ..
        }) = syntax
        else {
            return Some(None);
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
        Some(Some(DomainReferenceSystem {
            domain: self.bound_reference(owner, reference, target)?,
            axis,
        }))
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
                if !self.step(1, &reference.domain.span.clone()) {
                    return None;
                }
                let range = &reference_ranges(&self.types[id])[index];
                let reference = range.reference_system.as_ref().unwrap();
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
                    (Some((_, TypeInfo::Coordinate(coordinate))), None)
                        if coordinate.rotation.is_some() =>
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
                    (None, None) if reference.domain.target_name() == "INTERLIS.GregorianYear" => {
                        self.diagnostics.push(
                            "ILI-UNSUPPORTED",
                            "The predefined year domain requires its full definition",
                            Some(at),
                        );
                        return None;
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
                    let compatible = match &range.unit {
                        Some(actual) => unit_extends(
                            &self.symbols,
                            &self.names,
                            actual,
                            required,
                            &mut self.work,
                        )
                        .and_then(|extends| {
                            if extends {
                                Ok(true)
                            } else {
                                units_convertible(
                                    &self.symbols,
                                    &self.names,
                                    actual,
                                    required,
                                    &mut self.work,
                                )
                            }
                        }),
                        None => Ok(required.target_name() == "INTERLIS.ANYUNIT"),
                    };
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

fn reference_ranges(ty: &TypeInfo) -> &[NumericRange] {
    match ty {
        TypeInfo::Numeric(range) => std::slice::from_ref(range),
        TypeInfo::Coordinate(coordinate) => &coordinate.axes,
        TypeInfo::Oid(OidType::Typed(ty)) => reference_ranges(ty),
        _ => &[],
    }
}
