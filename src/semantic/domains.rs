use super::{
    Analyzer, DecimalLiteral, Effective, NumberError, RelatedLocation, TypeInfo, TypeSyntax,
};
use crate::{
    AxisRotation, CoordinateType, DecimalInfo, LanguageVersion, Limits, NumericBounds,
    NumericRange,
    limits::WorkBudget,
    syntax::{CoordinateSyntax, NumericSyntax},
};
use std::cmp::Ordering;

impl Analyzer<'_, '_> {
    pub(super) fn numeric_range(&mut self, syntax: &NumericSyntax) -> Option<NumericRange> {
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
        Some(NumericRange {
            bounds,
            circular: syntax.circular.is_some(),
            direction: syntax.direction.as_ref().map(|direction| direction.value),
        })
    }

    pub(super) fn coordinate_type(&mut self, syntax: &CoordinateSyntax) -> Option<CoordinateType> {
        let axes = syntax
            .axes
            .iter()
            .map(|axis| self.numeric_range(axis))
            .collect::<Option<Vec<_>>>()?;
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
        let check = scalar_extension(
            &self.types[own.ty],
            &self.types[base.ty],
            self.models[self.symbols[owner].model].language,
            self.limits,
            &mut self.work,
        );
        match check {
            Ok(None) => {
                let inherited_bytes = match (&self.types[own.ty], &self.types[base.ty]) {
                    (TypeInfo::Coordinate(own), TypeInfo::Coordinate(base))
                        if own.reference_system.is_none() =>
                    {
                        base.reference_system.as_ref().map_or(0, String::len)
                    }
                    _ => 0,
                };
                if !self.step(inherited_bytes, syntax.span()) {
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
                own.mandatory |= base.mandatory;
                Some(own)
            }
            Ok(Some((code, message))) => {
                let base = self.symbols[owner]
                    .declaration
                    .base_reference
                    .as_ref()
                    .expect("a domain extension has a bound base");
                self.diagnostics.push_related(
                    code,
                    message,
                    Some(syntax.span().clone()),
                    vec![RelatedLocation {
                        span: base.span.clone(),
                        message: "Base domain reference".into(),
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
}

fn scalar_extension(
    own: &TypeInfo,
    base: &TypeInfo,
    language: LanguageVersion,
    limits: &Limits,
    work: &mut WorkBudget,
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
        (Numeric(own), Numeric(base)) => {
            return numeric_extension(own, base, language, limits, work);
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
            {
                return Ok(Some((
                    "ILI-COORD-EXTENSION",
                    "A coordinate extension cannot change its base rotation or reference system",
                )));
            }
            for (own, base) in own.axes.iter().zip(&base.axes) {
                if let Some(reason) = numeric_extension(own, base, language, limits, work)? {
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
}

fn numeric_extension(
    own: &NumericRange,
    base: &NumericRange,
    language: LanguageVersion,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<Option<(&'static str, &'static str)>, NumberError> {
    if let Some(base_direction) = base.direction
        && own
            .direction
            .is_some_and(|direction| direction != base_direction)
    {
        return Ok(Some((
            "ILI-DOMAIN-DIRECTION",
            "The direction of a numeric base domain cannot change",
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
