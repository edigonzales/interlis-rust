use super::{Analyzer, Kind, LookupError, QualifiedName, Symbol, SyntaxDecl};
use crate::{
    BinaryOperator, DecimalInfo, Limits, NumericBounds, Span, TypeReference, UnitConstant,
    UnitDefinition, UnitFactor, UnitInfo, UnitOperator, UnitScale, UnitTerm,
    limits::WorkBudget,
    number::{DecimalLiteral, DivisionRounding, NumberError, constant_info},
    syntax,
};
use num_bigint::BigInt;
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
        match self.lookup_name(owner, &name.segments, &name.span, &[Kind::Unit]) {
            Ok(target) => self.bound_reference(owner, name, target),
            Err(LookupError::Limit) => None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "Unit reference cannot be resolved",
                    Some(name.span.clone()),
                );
                None
            }
        }
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

    pub(super) fn subdivision_unit_period(
        &mut self,
        predecessor: usize,
        subdivision: usize,
        span: &Span,
    ) -> Option<bool> {
        let (crate::TypeInfo::Numeric(predecessor), crate::TypeInfo::Numeric(subdivision)) =
            (&self.types[predecessor], &self.types[subdivision])
        else {
            unreachable!("a numeric subdivision and its predecessor have numeric types");
        };
        let (Some(coarse), Some(fine), Some(bounds)) =
            (&predecessor.unit, &subdivision.unit, &subdivision.bounds)
        else {
            return Some(true);
        };
        let matches = if self.unit_is_abstract(coarse) {
            unit_extends(&self.symbols, &self.names, fine, coarse, &mut self.work)
        } else {
            unit_period_matches(
                &self.symbols,
                &self.names,
                coarse,
                fine,
                bounds,
                &mut self.work,
            )
        };
        match matches {
            Ok(matches) => Some(matches),
            Err(error) => {
                self.number_error(error, span);
                None
            }
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

pub(super) fn units_convertible(
    symbols: &[Symbol<'_>],
    names: &BTreeMap<String, usize>,
    actual: &TypeReference,
    expected: &TypeReference,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    if actual.target_name() == expected.target_name() {
        return Ok(true);
    }
    let actual = unit_product(
        symbols,
        names,
        actual.target_name(),
        ProductKind::Roots,
        work,
    )?;
    let expected = unit_product(
        symbols,
        names,
        expected.target_name(),
        ProductKind::Roots,
        work,
    )?;
    Ok(actual == expected)
}

/// Evaluate unit factors in declaration order, as in the decimal evaluator.
pub(super) fn conversion_scales(
    symbols: &[Symbol<'_>],
    names: &BTreeMap<String, usize>,
    source: &TypeReference,
    destination: &TypeReference,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<Option<(DecimalLiteral, DecimalLiteral)>, NumberError> {
    let one = DecimalLiteral::parse("1", limits, work)?;
    if source.target_name() == destination.target_name() {
        return Ok(Some((one.clone(), one)));
    }
    let mut resolved: BTreeMap<&str, Option<DecimalLiteral>> = BTreeMap::new();
    let mut pending = vec![
        (source.target_name(), false),
        (destination.target_name(), false),
    ];
    while let Some((name, ready)) = pending.pop() {
        work.charge(1).map_err(|_| NumberError::Work)?;
        if resolved.contains_key(name) {
            continue;
        }
        let definition = declared_unit(symbols, names, name).map(|unit| &unit.definition);
        if !ready {
            let first = match definition {
                Some(UnitDefinition::Derived {
                    target,
                    scale: UnitScale::Factors(_) | UnitScale::Identity,
                }) => {
                    pending.push((name, true));
                    pending.push((target.target_name(), false));
                    continue;
                }
                Some(UnitDefinition::Composed { terms }) => {
                    pending.push((name, true));
                    pending.extend(terms.iter().map(|term| (term.unit.target_name(), false)));
                    continue;
                }
                Some(UnitDefinition::Derived {
                    scale: UnitScale::Function(_),
                    ..
                }) => None,
                _ => Some(match predefined_scale(name) {
                    Some(factor) => DecimalLiteral::parse(factor, limits, work)?,
                    None => one.clone(),
                }),
            };
            resolved.insert(name, first);
            continue;
        }
        let mut factor = match definition {
            Some(UnitDefinition::Derived { target, .. }) => resolved[target.target_name()].clone(),
            _ => Some(one.clone()),
        };
        match definition {
            Some(UnitDefinition::Derived {
                scale: UnitScale::Factors(factors),
                ..
            }) => {
                for term in factors {
                    let Some(current) = factor.take() else { break };
                    let argument = DecimalLiteral::from_info(constant_info(&term.value), work)?;
                    factor = Some(apply_scale(
                        current,
                        &argument,
                        term.operator,
                        limits,
                        work,
                    )?);
                }
            }
            Some(UnitDefinition::Composed { terms }) => {
                for term in terms {
                    let (Some(current), Some(argument)) =
                        (factor, &resolved[term.unit.target_name()])
                    else {
                        factor = None;
                        break;
                    };
                    factor = Some(apply_scale(current, argument, term.operator, limits, work)?);
                }
            }
            Some(UnitDefinition::Derived {
                scale: UnitScale::Identity,
                ..
            }) => {}
            _ => unreachable!("only dependent scales are deferred"),
        }
        resolved.insert(name, factor);
    }
    Ok(resolved
        .remove(source.target_name())
        .flatten()
        .zip(resolved.remove(destination.target_name()).flatten()))
}

fn apply_scale(
    current: DecimalLiteral,
    argument: &DecimalLiteral,
    operator: UnitOperator,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<DecimalLiteral, NumberError> {
    current.operate(
        match operator {
            UnitOperator::Multiply => BinaryOperator::Multiply,
            UnitOperator::Divide => BinaryOperator::Divide,
        },
        argument,
        DivisionRounding::Nearest,
        limits,
        work,
    )
}

fn predefined_scale(name: &str) -> Option<&'static str> {
    match name {
        "INTERLIS.min" => Some("60"),
        "INTERLIS.h" => Some("3600"),
        "INTERLIS.d" => Some("86400"),
        _ => None,
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ProductKind {
    Roots,
    Ratio,
    Dimension,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Factor<'a> {
    Unit(&'a str),
    Decimal(i32, &'a str),
    Pi,
    LnBase,
}

type Product<'a> = BTreeMap<Factor<'a>, BigInt>;

fn unit_product<'a>(
    symbols: &'a [Symbol<'_>],
    names: &BTreeMap<String, usize>,
    root: &'a str,
    kind: ProductKind,
    work: &mut WorkBudget,
) -> Result<Product<'a>, NumberError> {
    let mut resolved = BTreeMap::<&str, Product<'_>>::new();
    let mut pending = vec![(root, false)];
    while let Some((name, ready)) = pending.pop() {
        work.charge(1).map_err(|_| NumberError::Work)?;
        if resolved.contains_key(name) {
            continue;
        }
        let unit = declared_unit(symbols, names, name);
        let definition = unit.map(|unit| &unit.definition);
        let predefined_scale = predefined_scale(name);
        let alias = match (definition, kind) {
            (
                Some(UnitDefinition::Derived {
                    scale: UnitScale::Function(_),
                    ..
                }),
                ProductKind::Ratio,
            ) => None,
            (Some(UnitDefinition::Derived { target, .. }), _) => Some(target.target_name()),
            (Some(UnitDefinition::Fundamental), ProductKind::Dimension) => unit
                .and_then(|unit| unit.base.as_ref())
                .map(TypeReference::target_name)
                .filter(|&parent| parent != "INTERLIS.ANYUNIT"),
            (None, ProductKind::Dimension) => {
                predefined_parent(name).filter(|&parent| parent != "INTERLIS.ANYUNIT")
            }
            (None, _) if predefined_scale.is_some() => Some("INTERLIS.s"),
            _ => None,
        };
        let terms = match definition {
            Some(UnitDefinition::Composed { terms }) => terms.as_slice(),
            _ => &[],
        };
        if !ready && (alias.is_some() || !terms.is_empty()) {
            pending.push((name, true));
            pending.extend(alias.into_iter().map(|target| (target, false)));
            pending.extend(terms.iter().map(|term| (term.unit.target_name(), false)));
            continue;
        }
        let mut product = Product::new();
        if alias.is_none()
            && terms.is_empty()
            && !(kind == ProductKind::Dimension && name == "INTERLIS.DIMENSIONLESS")
        {
            product.insert(Factor::Unit(name), BigInt::from(1));
        }
        if kind == ProductKind::Ratio {
            if let Some(UnitDefinition::Derived {
                scale: UnitScale::Factors(factors),
                ..
            }) = definition
            {
                for factor in factors {
                    let key = match &factor.value {
                        UnitConstant::Decimal(value) => {
                            match decimal_factor(value.coefficient(), value.scale()) {
                                Some(key) => key,
                                None => continue,
                            }
                        }
                        UnitConstant::Pi => Factor::Pi,
                        UnitConstant::LnBase => Factor::LnBase,
                    };
                    add_power(&mut product, key, &BigInt::from(1), factor.operator, work)?;
                }
            }
            if let Some(key) = predefined_scale.and_then(|value| decimal_factor(value, 0)) {
                product.insert(key, BigInt::from(1));
            }
        }
        for (operator, target) in alias
            .into_iter()
            .map(|target| (UnitOperator::Multiply, target))
            .chain(
                terms
                    .iter()
                    .map(|term| (term.operator, term.unit.target_name())),
            )
        {
            for (&factor, exponent) in &resolved[target] {
                add_power(&mut product, factor, exponent, operator, work)?;
            }
        }
        product.retain(|_, power| power.bits() != 0);
        resolved.insert(name, product);
    }
    Ok(resolved
        .remove(root)
        .expect("every reachable conversion root was resolved"))
}

fn decimal_factor(coefficient: &str, scale: i32) -> Option<Factor<'_>> {
    let digits = coefficient.trim_end_matches('0');
    let zeros = i32::try_from(coefficient.len() - digits.len()).expect("bounded precision");
    let scale = scale - zeros;
    (digits != "1" || scale != 0).then_some(Factor::Decimal(scale, digits))
}

fn add_power<'a>(
    product: &mut Product<'a>,
    factor: Factor<'a>,
    exponent: &BigInt,
    operator: UnitOperator,
    work: &mut WorkBudget,
) -> Result<(), NumberError> {
    let power = product.entry(factor).or_default();
    charge_product_work(exponent.bits().max(power.bits()).div_ceil(8) + 1, work)?;
    match operator {
        UnitOperator::Multiply => *power += exponent,
        UnitOperator::Divide => *power -= exponent,
    }
    Ok(())
}

fn unit_period_matches(
    symbols: &[Symbol<'_>],
    names: &BTreeMap<String, usize>,
    coarse: &TypeReference,
    fine: &TypeReference,
    bounds: &NumericBounds,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    let coarse = coarse.target_name();
    let fine = fine.target_name();
    let mut ratio = unit_product(symbols, names, coarse, ProductKind::Ratio, work)?;
    for (factor, exponent) in unit_product(symbols, names, fine, ProductKind::Ratio, work)? {
        add_power(&mut ratio, factor, &exponent, UnitOperator::Divide, work)?;
    }
    ratio.retain(|_, power| power.bits() != 0);
    if ratio.keys().any(|factor| matches!(factor, Factor::Unit(_))) {
        let mut dimensions = unit_product(symbols, names, coarse, ProductKind::Dimension, work)?;
        for (factor, exponent) in unit_product(symbols, names, fine, ProductKind::Dimension, work)?
        {
            add_power(
                &mut dimensions,
                factor,
                &exponent,
                UnitOperator::Divide,
                work,
            )?;
        }
        dimensions.retain(|_, power| power.bits() != 0);
        return Ok(dimensions.is_empty()
            || dimensions.keys().any(|factor| {
                matches!(factor, Factor::Unit(name)
                    if *name == "INTERLIS.ANYUNIT"
                        || !name.starts_with("INTERLIS.")
                            && declared_unit(symbols, names, name).is_some_and(|unit| !unit.abstract_unit))
            }));
    }
    let symbolic = usize::from(ratio.contains_key(&Factor::Pi))
        + usize::from(ratio.contains_key(&Factor::LnBase));
    if symbolic != 0 {
        // A nonzero integral power of either constant is transcendental.
        // A product involving both constants has no established rational value.
        return Ok(symbolic == 2);
    }

    let mut numerator = BigInt::from(1);
    let mut denominator = BigInt::from(1);
    for (factor, power) in ratio {
        let Factor::Decimal(scale, coefficient) = factor else {
            unreachable!("unit roots and symbolic factors were checked");
        };
        let exponent = u32::try_from(power.magnitude()).map_err(|_| NumberError::Work)?;
        let base = parse_coefficient(coefficient);
        let decimal_places = scale
            .unsigned_abs()
            .checked_mul(exponent)
            .ok_or(NumberError::Work)?;
        let magnitude = exact_power(&base, exponent, work)?;
        let decimal = exact_power(&BigInt::from(10), decimal_places, work)?;
        let (top, bottom) = if scale >= 0 {
            (magnitude, decimal)
        } else {
            (exact_product(magnitude, &decimal, work)?, BigInt::from(1))
        };
        let (top, bottom) = if power.sign() == num_bigint::Sign::Minus {
            (bottom, top)
        } else {
            (top, bottom)
        };
        numerator = exact_product(numerator, &top, work)?;
        denominator = exact_product(denominator, &bottom, work)?;
    }

    let scale = bounds.lower.scale().max(bounds.upper.scale());
    let aligned = |bound: &DecimalInfo, work: &mut WorkBudget| {
        let shift = exact_power(&BigInt::from(10), scale.abs_diff(bound.scale()), work)?;
        exact_product(parse_coefficient(bound.coefficient()), &shift, work)
    };
    let period = aligned(&bounds.upper, work)? - aligned(&bounds.lower, work)?
        + exact_power(
            &BigInt::from(10),
            scale.abs_diff(bounds.upper.scale()),
            work,
        )?;
    let divisor = exact_power(&BigInt::from(10), scale.unsigned_abs(), work)?;
    if scale >= 0 {
        Ok(exact_product(numerator, &divisor, work)? == exact_product(denominator, &period, work)?)
    } else {
        Ok(numerator == exact_product(exact_product(denominator, &period, work)?, &divisor, work)?)
    }
}

fn parse_coefficient(value: &str) -> BigInt {
    value
        .parse()
        .expect("an exact decimal has a valid coefficient")
}

fn exact_power(base: &BigInt, exponent: u32, work: &mut WorkBudget) -> Result<BigInt, NumberError> {
    let bits = base.bits().saturating_mul(u64::from(exponent));
    charge_product_work(bits.saturating_mul(bits) / 4096 + 1, work)?;
    Ok(base.pow(exponent))
}

fn exact_product(
    left: BigInt,
    right: &BigInt,
    work: &mut WorkBudget,
) -> Result<BigInt, NumberError> {
    charge_product_work(left.bits().saturating_mul(right.bits()) / 4096 + 1, work)?;
    Ok(left * right)
}

fn charge_product_work(cost: u64, work: &mut WorkBudget) -> Result<(), NumberError> {
    work.charge(u32::try_from(cost).map_err(|_| NumberError::Work)?)
        .map_err(|_| NumberError::Work)
}
