use crate::{Limits, limits::WorkBudget, model::DecimalInfo};
use num_bigint::{BigInt, Sign};
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NumberError {
    Form,
    Limit,
    Work,
}
pub(crate) struct DecimalLiteral {
    pub info: DecimalInfo,
    value: BigInt,
}
impl DecimalLiteral {
    pub fn parse(text: &str, limits: &Limits, work: &mut WorkBudget) -> Result<Self, NumberError> {
        // Inspect operational bounds before bigint allocation or exponent alignment.
        let mut digits = 0_u32;
        for b in text.bytes() {
            if b.is_ascii_digit() {
                digits += 1;
                if digits > limits.max_numeric_digits {
                    return Err(NumberError::Limit);
                }
            }
        }
        work.charge(digits).map_err(|_| NumberError::Work)?;
        let (negative, unsigned) = if let Some(s) = text.strip_prefix('-') {
            (true, s)
        } else {
            (false, text.strip_prefix('+').unwrap_or(text))
        };
        let (mantissa, exponent_text) = unsigned.find(['e', 'E']).map_or((unsigned, None), |i| {
            (&unsigned[..i], Some(&unsigned[i + 1..]))
        });
        let mut exponent = 0_i32;
        if let Some(exp) = exponent_text {
            let (neg, exp) = if let Some(e) = exp.strip_prefix('-') {
                (true, e)
            } else {
                (false, exp.strip_prefix('+').unwrap_or(exp))
            };
            if exp.is_empty() || !exp.bytes().all(|b| b.is_ascii_digit()) {
                return Err(NumberError::Form);
            }
            let mut value = 0_u32;
            for b in exp.bytes() {
                value = value
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(u32::from(b - b'0')))
                    .ok_or(NumberError::Limit)?;
                if value > limits.max_abs_exponent {
                    return Err(NumberError::Limit);
                }
            }
            exponent = i32::try_from(value).map_err(|_| NumberError::Limit)?;
            if neg {
                exponent = -exponent;
            }
        }
        let (integer, fraction) = mantissa
            .split_once('.')
            .map_or((mantissa, None), |(a, b)| (a, Some(b)));
        if integer.is_empty() || !integer.bytes().all(|b| b.is_ascii_digit()) {
            return Err(NumberError::Form);
        }
        if fraction.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
            return Err(NumberError::Form);
        }
        if exponent_text.is_some() {
            let Some(f) = fraction else {
                return Err(NumberError::Form);
            };
            // Original formal Float production; broad lexer spellings do not certify it.
            if integer != "0"
                || (!f.starts_with(|c: char| c.is_ascii_digit() && c != '0')
                    && !f.bytes().all(|b| b == b'0'))
            {
                return Err(NumberError::Form);
            }
        }
        let fractional_digits =
            u32::try_from(fraction.map_or(0, str::len)).map_err(|_| NumberError::Limit)?;
        let mut coefficient = String::with_capacity(mantissa.len());
        coefficient.push_str(integer);
        if let Some(f) = fraction {
            coefficient.push_str(f)
        }
        let mut value = BigInt::parse_bytes(coefficient.as_bytes(), 10).ok_or(NumberError::Form)?;
        if negative {
            value = -value;
        }
        Ok(Self {
            info: DecimalInfo {
                original: text.into(),
                coefficient: value.to_string(),
                fractional_digits,
                exponent,
                scaled: exponent_text.is_some(),
            },
            value,
        })
    }
    pub fn is_positive_integer(&self) -> bool {
        self.value.sign() == Sign::Plus && self.info.fractional_digits == 0 && !self.info.scaled
    }
    pub fn compare_value(
        &self,
        other: &Self,
        work: &mut WorkBudget,
    ) -> Result<Ordering, NumberError> {
        if self.value.sign() != other.value.sign() {
            return Ok(self.value.sign().cmp(&other.value.sign()));
        }
        if self.value.sign() == Sign::NoSign {
            return Ok(Ordering::Equal);
        }
        let a = self.info.scale();
        let b = other.info.scale();
        let distance = a.abs_diff(b);
        work.charge(distance).map_err(|_| NumberError::Work)?;
        let power = BigInt::from(10_u8).pow(distance);
        Ok(if a < b {
            (&self.value * power).cmp(&other.value)
        } else {
            self.value.cmp(&(&other.value * power))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equal_values_retain_different_written_precision_and_scale() {
        let limits = Limits::default();
        let mut work = WorkBudget::new(&limits);
        let a = DecimalLiteral::parse("1.0", &limits, &mut work).unwrap();
        let b = DecimalLiteral::parse("1.00", &limits, &mut work).unwrap();
        assert_eq!(a.compare_value(&b, &mut work), Ok(Ordering::Equal));
        assert_eq!(
            (a.info.fractional_digits(), b.info.fractional_digits()),
            (1, 2)
        );
        assert_eq!((a.info.original(), b.info.original()), ("1.0", "1.00"));
        let a = DecimalLiteral::parse("-0.10e+1", &limits, &mut work).unwrap();
        let b = DecimalLiteral::parse("-1.00", &limits, &mut work).unwrap();
        assert_eq!(a.compare_value(&b, &mut work), Ok(Ordering::Equal));
        assert_eq!((a.info.written_exponent(), a.info.scale()), (1, 1));
        let a = DecimalLiteral::parse("-9007199254740993", &limits, &mut work).unwrap();
        let b = DecimalLiteral::parse("-9007199254740992", &limits, &mut work).unwrap();
        assert_eq!(a.compare_value(&b, &mut work), Ok(Ordering::Less));
    }
    #[test]
    fn norm_float_forms_and_budgets_precede_alignment() {
        let limits = Limits::default();
        let mut work = WorkBudget::new(&limits);
        for text in ["1E2", "1.0e2", "0.001e2", "- 1", "0.1e", ""] {
            assert!(
                matches!(
                    DecimalLiteral::parse(text, &limits, &mut work),
                    Err(NumberError::Form)
                ),
                "{text}"
            );
        }
        for text in ["0e1000000000", "0.0e-1000000000"] {
            assert!(matches!(
                DecimalLiteral::parse(text, &limits, &mut work),
                Err(NumberError::Limit)
            ));
        }
        let a = DecimalLiteral::parse("0.1e-1000", &limits, &mut work).unwrap();
        let b = DecimalLiteral::parse("0.1e1000", &limits, &mut work).unwrap();
        let mut tiny = WorkBudget::new(&Limits {
            max_semantic_steps: 1,
            ..limits
        });
        assert_eq!(a.compare_value(&b, &mut tiny), Err(NumberError::Work));
    }
}
