use super::{Analyzer, BoundName, Kind, LookupError, QualifiedName, Symbol, SyntaxDecl};
use crate::{
    TypeReference, UnitConstant, UnitDefinition, UnitFactor, UnitInfo, UnitOperator, UnitScale,
    UnitTerm, limits::WorkBudget, number::NumberError, syntax,
};
use std::collections::{BTreeMap, BTreeSet};

impl Analyzer<'_, '_> {
    pub(super) fn bind_units(&mut self) -> Option<()> {
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::Unit(unit)) = self.symbols[id].ast else {
                continue;
            };
            let info = self.unit_info(id, unit)?;
            self.symbols[id].declaration.abstract_declaration = info.abstract_unit;
            self.symbols[id].declaration.unit = Some(Box::new(info));
        }
        self.check_unit_cycles()?;
        for id in 0..self.symbols.len() {
            let Some(unit) = self.symbols[id].declaration.unit() else {
                continue;
            };
            let definition = &unit.definition;
            let base = unit.base.as_ref();
            if let Some(base) = base
                && !self.unit_is_abstract(base)
            {
                self.diagnostics.push(
                    "ILI-UNIT-BASE",
                    "The base of a unit must be abstract",
                    Some(base.span.clone()),
                );
                return None;
            }
            let span = self.symbols[id].name_span.clone();
            match definition {
                UnitDefinition::Fundamental => {}
                UnitDefinition::Derived { target, .. } => {
                    if unit.abstract_unit || self.unit_is_abstract(target) {
                        self.diagnostics.push(
                            "ILI-UNIT-DERIVED",
                            "A conversion must link two concrete units",
                            Some(target.span.clone()),
                        );
                        return None;
                    }
                    if let Some(base) = base {
                        let extends =
                            unit_extends(&self.symbols, &self.names, target, base, &mut self.work);
                        match extends {
                            Ok(true) => {}
                            Ok(false) => self.diagnostics.push(
                                "ILI-UNIT-EXTENSION",
                                "The conversion target must specialize the abstract base unit",
                                Some(target.span.clone()),
                            ),
                            Err(error) => {
                                let location = target.span.clone();
                                self.number_error(error, &location);
                            }
                        }
                        if self.diagnostics.limited || self.diagnostics.rejected {
                            return None;
                        }
                    }
                }
                UnitDefinition::Composed { terms } => {
                    for term in terms {
                        if self.unit_is_abstract(&term.unit) != unit.abstract_unit {
                            self.diagnostics.push(
                                "ILI-UNIT-COMPONENT",
                                "The composed unit and its components must agree on abstraction",
                                Some(term.unit.span.clone()),
                            );
                            return None;
                        }
                    }
                    if let Some(base) = base {
                        let abstract_terms =
                            declared_unit(&self.symbols, &self.names, base.target_name())
                                .and_then(|unit| match &unit.definition {
                                    UnitDefinition::Composed { terms } => Some(terms.as_slice()),
                                    _ => None,
                                })
                                .unwrap_or_default();
                        let mut compatible = terms.len() == abstract_terms.len();
                        for (term, expected) in terms.iter().zip(abstract_terms) {
                            let extends = unit_extends(
                                &self.symbols,
                                &self.names,
                                &term.unit,
                                &expected.unit,
                                &mut self.work,
                            );
                            match extends {
                                Ok(extends) => {
                                    compatible &= term.operator == expected.operator && extends
                                }
                                Err(error) => {
                                    let location = term.unit.span.clone();
                                    self.number_error(error, &location);
                                    return None;
                                }
                            }
                        }
                        if !compatible {
                            self.diagnostics.push(
                                "ILI-UNIT-EXTENSION",
                                "A composed unit must preserve its base expression and specialize each component",
                                Some(span),
                            );
                            return None;
                        }
                    }
                }
            }
        }
        Some(())
    }

    fn unit_info(&mut self, owner: usize, syntax: &syntax::UnitDecl) -> Option<UnitInfo> {
        let base = match &syntax.base {
            Some(name) => Some(self.bind_unit_reference(owner, name)?),
            None => None,
        };
        let definition = match &syntax.definition {
            None => UnitDefinition::Fundamental,
            Some(syntax::UnitDefinition::Derived { scale, unit, .. }) => {
                let target = self.bind_unit_reference(owner, unit)?;
                let scale = match scale {
                    None => UnitScale::Identity,
                    Some(syntax::UnitScale::Function(function)) => {
                        if !self.step(function.value.len(), &function.span) {
                            return None;
                        }
                        UnitScale::Function(function.value.clone())
                    }
                    Some(syntax::UnitScale::Factors { first, rest }) => {
                        let factors = std::iter::once((UnitOperator::Multiply, first))
                            .chain(rest.iter().map(|(op, value)| (op.value, value)))
                            .map(|(operator, constant)| {
                                Some(UnitFactor {
                                    operator,
                                    value: self.unit_constant(constant)?,
                                })
                            })
                            .collect::<Option<Vec<_>>>()?;
                        UnitScale::Factors(factors)
                    }
                };
                UnitDefinition::Derived { target, scale }
            }
            Some(syntax::UnitDefinition::Composed { first, rest, .. }) => {
                let terms = std::iter::once((UnitOperator::Multiply, first))
                    .chain(rest.iter().map(|(op, name)| (op.value, name)))
                    .map(|(operator, name)| {
                        Some(UnitTerm {
                            operator,
                            unit: self.bind_unit_reference(owner, name)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                UnitDefinition::Composed { terms }
            }
        };
        Some(UnitInfo {
            designation: syntax.name.value.clone(),
            abstract_unit: matches!(syntax.display, Some(syntax::UnitDisplay::Abstract(_))),
            base,
            definition,
        })
    }

    pub(super) fn bind_unit_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
    ) -> Option<TypeReference> {
        let (target, predefined_language) =
            match self.lookup_name(owner, &name.segments, &name.span, &[Kind::Unit]) {
                Ok(BoundName::Declared(id)) => {
                    (self.symbols[id].declaration.qualified.clone(), None)
                }
                Ok(BoundName::Predefined(name, _)) => (
                    format!("INTERLIS.{name}"),
                    Some(self.models[self.symbols[owner].model].language),
                ),
                Err(LookupError::Limit) => return None,
                Err(_) => {
                    self.diagnostics.push(
                        "ILI-NAME-NOT-FOUND",
                        "Unit reference cannot be resolved",
                        Some(name.span.clone()),
                    );
                    return None;
                }
            };
        Some(TypeReference {
            declared: name
                .segments
                .iter()
                .map(|part| part.value.as_str())
                .collect::<Vec<_>>()
                .join("."),
            target,
            predefined_language,
            span: name.span.clone(),
        })
    }

    fn unit_constant(&mut self, constant: &syntax::DecimalConstant) -> Option<UnitConstant> {
        match constant {
            syntax::DecimalConstant::Pi(span) => self.step(1, span).then_some(UnitConstant::Pi),
            syntax::DecimalConstant::LnBase(span) => {
                self.step(1, span).then_some(UnitConstant::LnBase)
            }
            syntax::DecimalConstant::Number(number) => {
                let value = self.literal(number)?.info;
                if value.coefficient() == "0" {
                    self.diagnostics.push(
                        "ILI-UNIT-ZERO",
                        "A unit conversion factor must be nonzero",
                        Some(number.span.clone()),
                    );
                    return None;
                }
                Some(UnitConstant::Decimal(value))
            }
        }
    }

    pub(super) fn unit_is_abstract(&self, reference: &TypeReference) -> bool {
        if let Some(unit) = declared_unit(&self.symbols, &self.names, reference.target_name()) {
            unit.abstract_unit
        } else {
            reference.target_name() == "INTERLIS.ANYUNIT"
                || predefined_parent(reference.target_name()) == Some("INTERLIS.ANYUNIT")
        }
    }

    fn check_unit_cycles(&mut self) -> Option<()> {
        let units: Vec<_> = self
            .symbols
            .iter()
            .enumerate()
            .filter_map(|(id, symbol)| symbol.declaration.unit().map(|unit| (id, unit)))
            .collect();
        if units.is_empty() {
            return Some(());
        }
        let mut dependents = vec![Vec::new(); self.symbols.len()];
        let mut pending = vec![0; self.symbols.len()];
        for &(id, unit) in &units {
            let definition_edges = match &unit.definition {
                UnitDefinition::Composed { terms } => terms.as_slice(),
                _ => &[],
            };
            let converted = match &unit.definition {
                UnitDefinition::Derived { target, .. } => Some(target),
                _ => None,
            };
            for target in unit
                .base
                .iter()
                .chain(converted)
                .chain(definition_edges.iter().map(|term| &term.unit))
            {
                if self.work.charge(1).is_err() {
                    self.diagnostics
                        .limit("Semantic work budget exceeded", Some(target.span.clone()));
                    return None;
                }
                if let Some(&dependency) = self.names.get(target.target_name()) {
                    dependents[dependency].push(id);
                    pending[id] += 1;
                }
            }
        }
        let mut ready: Vec<_> = units
            .iter()
            .map(|&(id, _)| id)
            .filter(|&id| pending[id] == 0)
            .collect();
        while let Some(id) = ready.pop() {
            for &dependent in &dependents[id] {
                pending[dependent] -= 1;
                if pending[dependent] == 0 {
                    ready.push(dependent);
                }
            }
        }
        if let Some(&(id, _)) = units.iter().find(|&&(id, _)| pending[id] != 0) {
            self.diagnostics.push(
                "ILI-UNIT-CYCLE",
                "The unit depends on a cyclic unit definition",
                Some(self.symbols[id].name_span.clone()),
            );
            return None;
        }
        Some(())
    }
}

fn predefined_parent(name: &str) -> Option<&'static str> {
    Some(match name.strip_prefix("INTERLIS.")? {
        "DIMENSIONLESS" | "LENGTH" | "MASS" | "TIME" | "ELECTRIC_CURRENT" | "TEMPERATURE"
        | "AMOUNT_OF_MATTER" | "ANGLE" | "SOLID_ANGLE" | "LUMINOUS_INTENSITY" | "MONEY" => {
            "INTERLIS.ANYUNIT"
        }
        "m" => "INTERLIS.LENGTH",
        "kg" => "INTERLIS.MASS",
        "s" | "min" | "h" | "d" | "M" | "Y" => "INTERLIS.TIME",
        "A" => "INTERLIS.ELECTRIC_CURRENT",
        "K" => "INTERLIS.TEMPERATURE",
        "mol" => "INTERLIS.AMOUNT_OF_MATTER",
        "rad" => "INTERLIS.ANGLE",
        "sr" => "INTERLIS.SOLID_ANGLE",
        "cd" => "INTERLIS.LUMINOUS_INTENSITY",
        _ => return None,
    })
}

fn declared_unit<'a>(
    symbols: &'a [Symbol<'_>],
    names: &BTreeMap<String, usize>,
    name: &str,
) -> Option<&'a UnitInfo> {
    symbols[*names.get(name)?].declaration.unit()
}

pub(super) fn unit_extends(
    symbols: &[Symbol<'_>],
    names: &BTreeMap<String, usize>,
    actual: &TypeReference,
    expected: &TypeReference,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    if expected.target_name() == "INTERLIS.ANYUNIT" {
        return Ok(true);
    }
    let mut pending = vec![actual.target_name()];
    let mut seen = BTreeSet::new();
    while let Some(name) = pending.pop() {
        let charge = u32::try_from(name.len().saturating_add(1)).map_err(|_| NumberError::Work)?;
        work.charge(charge).map_err(|_| NumberError::Work)?;
        if name == expected.target_name() {
            return Ok(true);
        }
        if !seen.insert(name) {
            continue;
        }
        if let Some(unit) = declared_unit(symbols, names, name) {
            pending.extend(unit.base.iter().map(TypeReference::target_name));
            if let UnitDefinition::Derived { target, .. } = &unit.definition {
                pending.push(target.target_name());
            }
        } else {
            pending.extend(predefined_parent(name));
        }
    }
    Ok(false)
}
