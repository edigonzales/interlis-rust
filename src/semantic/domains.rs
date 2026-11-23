use super::{
    Analyzer, DecimalLiteral, Effective, NumberError, RelatedLocation, TypeInfo, TypeSyntax,
};
use crate::{
    DecimalInfo, LanguageVersion, Limits, NumericBounds, NumericRange, limits::WorkBudget,
    syntax::NumericSyntax,
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
                if let TypeInfo::Numeric(range) = &self.types[base.ty] {
                    let flags = (range.circular, range.direction);
                    if let TypeInfo::Numeric(own_range) = &mut self.types[own.ty] {
                        own_range.circular |= flags.0;
                        own_range.direction = own_range.direction.or(flags.1);
                    }
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
            match (&own.bounds, &base.bounds) {
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
            }
        }
        _ => false,
    };
    Ok((!narrower).then_some((
        "ILI-DOMAIN-EXTENSION",
        "The type does not specialize the values of its base domain",
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
