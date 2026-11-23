use super::{
    Analyzer, DecimalLiteral, Effective, NumberError, RelatedLocation, TypeInfo, TypeSyntax,
    units::unit_extends,
};
use crate::{
    AxisRotation, CoordinateType, DecimalInfo, LanguageVersion, Limits, NumericBounds,
    NumericRange, OidType,
    limits::WorkBudget,
    syntax::{CoordinateSyntax, NumericSyntax},
};
use std::cmp::Ordering;

impl Analyzer<'_, '_> {
    pub(super) fn numeric_range(
        &mut self,
        owner: usize,
        syntax: &NumericSyntax,
    ) -> Option<NumericRange> {
        if !self.step(1, &syntax.span) {
            return None;
        }
        let bounds = if let Some(range) = &syntax.bounds {
            let low = self.literal(&range.lower)?;
            let high = self.literal(&range.upper)?;
            if low.info.fractional_digits() != high.info.fractional_digits() {
                self.diagnostics.push_related(
                    "ILI-NUMERIC-PRECISION",
                    "Numeric bounds must have matching written fractional precision",
                    Some(range.upper.span.clone()),
                    vec![RelatedLocation {
                        span: range.lower.span.clone(),
                        message: "Lower-bound precision".into(),
                    }],
                );
                return None;
            }
            match low.compare_value(&high, &mut self.work) {
                Ok(Ordering::Greater) => {
                    self.diagnostics.push_related(
                        "ILI-NUMERIC-RANGE",
                        "Lower numeric bound exceeds upper bound",
                        Some(range.lower.span.clone()),
                        vec![RelatedLocation {
                            span: range.upper.span.clone(),
                            message: "Upper bound".into(),
                        }],
                    );
                    return None;
                }
                Err(error) => {
                    self.number_error(error, &range.lower.span);
                    return None;
                }
                _ => {}
            }
            Some(NumericBounds {
                lower: low.info,
                upper: high.info,
            })
        } else {
            None
        };
        let unit = if let Some(name) = &syntax.unit {
            let unit = self.bind_unit_reference(owner, name)?;
            if bounds.is_some() && self.unit_is_abstract(&unit) {
                self.diagnostics.push(
                    "ILI-NUMERIC-UNIT-ABSTRACT",
                    "A bounded numeric range requires a concrete unit",
                    Some(name.span.clone()),
                );
                return None;
            }
            Some(unit)
        } else {
            None
        };
        Some(NumericRange {
            bounds,
            unit,
            circular: syntax.circular.is_some(),
            direction: syntax.direction.as_ref().map(|direction| direction.value),
        })
    }

    pub(super) fn coordinate_type(
        &mut self,
        owner: usize,
        syntax: &CoordinateSyntax,
    ) -> Option<CoordinateType> {
        let axes = syntax
            .axes
            .iter()
            .map(|axis| self.numeric_range(owner, axis))
            .collect::<Option<Vec<_>>>()?;
        if let Some(unit) = axes
            .iter()
            .filter_map(|axis| axis.unit.as_ref())
            .find(|unit| self.unit_is_abstract(unit))
        {
            self.diagnostics.push(
                "ILI-COORD-UNIT-ABSTRACT",
                "Coordinate axes require concrete units",
                Some(unit.span.clone()),
            );
            return None;
        }
        let rotation = if let Some(rotation) = &syntax.rotation {
            let axis = |spelling: &str| {
                spelling
                    .parse::<u8>()
                    .ok()
                    .filter(|&axis| axis > 0 && usize::from(axis) <= axes.len())
            };
            let zero_axis = axis(&rotation.zero.value);
            let quarter_turn_axis = axis(&rotation.quarter_turn.value);
            if let (Some(zero_axis), Some(quarter_turn_axis)) = (zero_axis, quarter_turn_axis)
                && zero_axis != quarter_turn_axis
            {
                Some(AxisRotation {
                    zero_axis,
                    quarter_turn_axis,
                })
            } else {
                self.diagnostics.push(
                    "ILI-COORD-ROTATION",
                    "Rotation requires two distinct existing axes",
                    Some(rotation.span.clone()),
                );
                return None;
            }
        } else {
            None
        };
        let reference_system = if let Some(crs) = &syntax.crs {
            if !self.step(crs.value.len(), &crs.span) {
                return None;
            }
            if !crs.value.strip_prefix("EPSG:").is_some_and(|code| {
                !code.is_empty() && code.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                self.diagnostics.push(
                    "ILI-COORD-REFSYS",
                    "Coordinate reference system must match EPSG:PosNumber",
                    Some(crs.span.clone()),
                );
                return None;
            }
            Some(crs.value.clone())
        } else {
            None
        };
        Some(CoordinateType {
            kind: syntax.kind,
            axes,
            rotation,
            reference_system,
        })
    }

    pub(super) fn specialize_scalar(
        &mut self,
        owner: usize,
        base: Effective,
        syntax: &TypeSyntax,
    ) -> Option<Effective> {
        let mut own = self.concrete_type(owner, syntax)?;
        own.mandatory |= base.mandatory;
        if own.ty == base.ty {
            return Some(own);
        }
        let inline = !matches!(syntax, TypeSyntax::Named(_));
        if !inline
            && matches!(self.types[own.ty], TypeInfo::Enumeration { .. })
            && matches!(self.types[base.ty], TypeInfo::Enumeration { .. })
            && self.named_enumeration_refines(owner, base.ty, syntax.span())?
        {
            return Some(own);
        }
        let check = self
            .check_unit_specialization(own.ty, base.ty, inline)
            .and_then(|unit_check| {
                if unit_check.is_some() {
                    return Ok(unit_check);
                }
                scalar_extension(
                    &self.types[own.ty],
                    &self.types[base.ty],
                    self.models[self.symbols[owner].model].language,
                    self.limits,
                    &mut self.work,
                    inline,
                )
            });
        match check {
            Ok(None) if !inline => Some(own),
            Ok(None) => {
                let own_type = &self.types[own.ty];
                let base_type = &self.types[base.ty];
                let unit_bytes = numeric_pairs(own_type, base_type)
                    .filter_map(|(own, base)| {
                        own.unit.is_none().then_some(base.unit.as_ref()).flatten()
                    })
                    .map(|unit| unit.declared.len() + unit.target.len())
                    .sum::<usize>();
                let crs_bytes = match (own_type, base_type) {
                    (TypeInfo::Coordinate(own), TypeInfo::Coordinate(base))
                        if own.reference_system.is_none() =>
                    {
                        base.reference_system.as_ref().map_or(0, String::len)
                    }
                    _ => 0,
                };
                if !self.step(unit_bytes.saturating_add(crs_bytes), syntax.span()) {
                    return None;
                }
                let [own_type, base_type] = self
                    .types
                    .get_disjoint_mut([own.ty, base.ty])
                    .expect("a specialization owns a new type");
                match (own_type, base_type) {
                    (TypeInfo::Numeric(own), TypeInfo::Numeric(base)) => inherit_range(own, base),
                    (TypeInfo::Coordinate(own), TypeInfo::Coordinate(base)) => {
                        for (own, base) in own.axes.iter_mut().zip(&base.axes) {
                            inherit_range(own, base);
                        }
                        own.rotation = own.rotation.or(base.rotation);
                        if own.reference_system.is_none() {
                            own.reference_system.clone_from(&base.reference_system);
                        }
                    }
                    _ => {}
                }
                Some(own)
            }
            Ok(Some((code, message))) => {
                let base = self.symbols[owner]
                    .declaration
                    .base_reference
                    .as_ref()
                    .expect("a value type extension has a bound base");
                self.diagnostics.push_related(
                    code,
                    message,
                    Some(syntax.span().clone()),
                    vec![RelatedLocation {
                        span: base.span.clone(),
                        message: "Base type reference".into(),
                    }],
                );
                None
            }
            Err(error) => {
                self.number_error(error, syntax.span());
                None
            }
        }
    }

    fn named_enumeration_refines(
        &mut self,
        owner: usize,
        base_type: usize,
        span: &crate::Span,
    ) -> Option<bool> {
        let declaration = &self.symbols[owner].declaration;
        let mut target = declaration
            .reference
            .as_ref()
            .and_then(|link| self.names.get(link.target_name()))
            .copied();
        while let Some(id) = target {
            if !self.step(1, span) {
                return None;
            }
            let declaration = &self.symbols[id].declaration;
            if declaration.type_id == Some(base_type) {
                return Some(true);
            }
            target = declaration
                .base_reference()
                .and_then(|reference| self.names.get(reference.target_name()))
                .copied();
        }
        Some(false)
    }

    fn check_unit_specialization(
        &mut self,
        own: usize,
        base: usize,
        inline: bool,
    ) -> Result<Option<(&'static str, &'static str)>, NumberError> {
        for (own, base) in numeric_pairs(&self.types[own], &self.types[base]) {
            match (&own.unit, &base.unit) {
                (Some(_), None) if base.bounds.is_some() => {
                    return Ok(Some((
                        "ILI-NUMERIC-UNIT-EXTENSION",
                        "A bounded unitless base cannot gain a unit in an extension",
                    )));
                }
                (None, Some(base)) if own.bounds.is_some() && self.unit_is_abstract(base) => {
                    return Ok(Some((
                        "ILI-NUMERIC-UNIT-ABSTRACT",
                        "A bounded extension must specialize its inherited abstract unit with a concrete unit",
                    )));
                }
                (None, Some(_)) if !inline => {
                    return Ok(Some((
                        "ILI-NUMERIC-UNIT-EXTENSION",
                        "A named domain used in an override must preserve the base unit",
                    )));
                }
                (Some(own), Some(base))
                    if own.target_name() != base.target_name()
                        && (!self.unit_is_abstract(base)
                            || !unit_extends(
                                &self.symbols,
                                &self.names,
                                own,
                                base,
                                &mut self.work,
                            )?) =>
                {
                    return Ok(Some((
                        "ILI-NUMERIC-UNIT-EXTENSION",
                        "A unit change is only allowed when specializing an abstract base unit",
                    )));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

fn numeric_pairs<'a>(
    own: &'a TypeInfo,
    base: &'a TypeInfo,
) -> impl Iterator<Item = (&'a NumericRange, &'a NumericRange)> {
    let (own, base) = match (own, base) {
        (TypeInfo::Numeric(own), TypeInfo::Numeric(base)) => {
            (std::slice::from_ref(own), std::slice::from_ref(base))
        }
        (TypeInfo::Coordinate(own), TypeInfo::Coordinate(base)) => {
            (own.axes.as_slice(), base.axes.as_slice())
        }
        _ => ([].as_slice(), [].as_slice()),
    };
    own.iter().zip(base)
}

fn scalar_extension(
    own: &TypeInfo,
    base: &TypeInfo,
    language: LanguageVersion,
    limits: &Limits,
    work: &mut WorkBudget,
    inline: bool,
) -> Result<Option<(&'static str, &'static str)>, NumberError> {
    use TypeInfo::*;
    let narrower = match (own, base) {
        (Text { max_length }, Text { max_length: cap }) => {
            text_fits(Some(max_length), Some(cap), work)?
        }
        (Text { .. } | UnboundedText, UnboundedText) => true,
        (MultilineText { max_length }, MultilineText { max_length: cap }) => {
            text_fits(max_length.as_deref(), cap.as_deref(), work)?
        }
        (FixedText { format: a, .. }, FixedText { format: b, .. }) => a == b,
        (Boolean, Boolean) => true,
        (Alignment(a), Alignment(b)) => a == b,
        (Blackbox(a), Blackbox(b)) => a == b,
        (Temporal(a), Temporal(b)) => a == b,
        (EnumTree { domain: a }, EnumTree { domain: b }) => a == b,
        (Oid(OidType::Typed(carrier)), Oid(OidType::Any)) if !carrier.has_unbounded_range() => true,
        (Oid(_), _) | (_, Oid(_)) => {
            return Ok(Some((
                "ILI-OID-EXTENSION",
                "An OID domain extension must specialize an open OID with a concrete definition",
            )));
        }
        (Numeric(own), Numeric(base)) => {
            return numeric_extension(own, base, language, limits, work, inline);
        }
        (Coordinate(own), Coordinate(base)) => {
            if own.kind != base.kind || own.axes.len() < base.axes.len() {
                return Ok(Some((
                    "ILI-COORD-EXTENSION",
                    "A coordinate extension must preserve its kind and existing axes",
                )));
            }
            if own
                .rotation
                .zip(base.rotation)
                .is_some_and(|(own, base)| own != base)
                || own
                    .reference_system
                    .as_ref()
                    .zip(base.reference_system.as_ref())
                    .is_some_and(|(own, base)| own != base)
                || !inline
                    && (base.rotation.is_some() && own.rotation.is_none()
                        || base.reference_system.is_some() && own.reference_system.is_none())
            {
                return Ok(Some((
                    "ILI-COORD-EXTENSION",
                    "A coordinate extension cannot change its base rotation or reference system",
                )));
            }
            for (own, base) in own.axes.iter().zip(&base.axes) {
                if let Some(reason) = numeric_extension(own, base, language, limits, work, inline)?
                {
                    return Ok(Some(reason));
                }
            }
            true
        }
        _ => false,
    };
    Ok((!narrower).then_some((
        "ILI-DOMAIN-EXTENSION",
        "The type does not specialize the values of its base domain",
    )))
}

fn inherit_range(own: &mut NumericRange, base: &NumericRange) {
    own.circular |= base.circular;
    own.direction = own.direction.or(base.direction);
    if own.unit.is_none() {
        own.unit.clone_from(&base.unit);
    }
}

fn numeric_extension(
    own: &NumericRange,
    base: &NumericRange,
    language: LanguageVersion,
    limits: &Limits,
    work: &mut WorkBudget,
    inline: bool,
) -> Result<Option<(&'static str, &'static str)>, NumberError> {
    if let Some(base_direction) = base.direction
        && own
            .direction
            .map_or(!inline, |direction| direction != base_direction)
    {
        return Ok(Some((
            "ILI-DOMAIN-DIRECTION",
            "The direction of a numeric base domain cannot change",
        )));
    }
    if !inline && base.circular && !own.circular {
        return Ok(Some((
            "ILI-DOMAIN-DIRECTION",
            "A named domain used in an override must preserve circularity",
        )));
    }
    let narrower = match (&own.bounds, &base.bounds) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(own), Some(base)) => {
            let (lower, upper) = (&own.lower, &own.upper);
            let (min, max) = (&base.lower, &base.upper);
            if language == LanguageVersion::V24
                && (lower.fractional_digits() != min.fractional_digits()
                    || upper.fractional_digits() != max.fractional_digits())
            {
                return Ok(Some((
                    "ILI-DOMAIN-PRECISION",
                    "An INTERLIS 2.4 numeric extension must preserve the precision of its base",
                )));
            }
            compare(lower, min, language, limits, work)? != Ordering::Less
                && compare(upper, max, language, limits, work)? != Ordering::Greater
        }
    };
    Ok((!narrower).then_some((
        "ILI-DOMAIN-EXTENSION",
        "The numeric range widens its base domain",
    )))
}

fn text_fits(
    own: Option<&str>,
    base: Option<&str>,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    Ok(match (own, base) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(own), Some(base)) => {
            let work_units = u32::try_from(own.len().saturating_add(base.len()))
                .map_err(|_| NumberError::Work)?;
            work.charge(work_units).map_err(|_| NumberError::Work)?;
            let own = own.trim_start_matches(['+', '0']);
            let base = base.trim_start_matches(['+', '0']);
            own.len().cmp(&base.len()).then_with(|| own.cmp(base)) != Ordering::Greater
        }
    })
}

fn compare(
    a: &DecimalInfo,
    b: &DecimalInfo,
    language: LanguageVersion,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<Ordering, NumberError> {
    let a = DecimalLiteral::parse(a.original(), limits, work)?;
    let b = DecimalLiteral::parse(b.original(), limits, work)?;
    // INTERLIS 2.3 §2.8.5 compares rounded bounds. In 2.4 §3.8.5 the
    // precision is preserved and bounds compare exactly.
    match language {
        LanguageVersion::V23 => a.compare_rounded_to(&b, work),
        LanguageVersion::V24 => a.compare_value(&b, work),
    }
}
