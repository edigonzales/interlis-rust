use crate::{BinaryOperator, Limits, NumericBounds, limits::WorkBudget, model::DecimalInfo};
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
struct Spelling<'a> {
    negative: bool,
    integer: &'a str,
    fraction: Option<&'a str>,
    exponent: i32,
    scaled: bool,
    digits: u32,
}

// Shared lexical/form inspection. No bigint, power calculation or allocation.
fn inspect<'a>(
    text: &'a str,
    limits: &Limits,
    formatted: bool,
) -> Result<Spelling<'a>, NumberError> {
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
    if exponent_text.is_some() && !formatted {
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
    Ok(Spelling {
        negative,
        integer,
        fraction,
        exponent,
        scaled: exponent_text.is_some(),
        digits,
    })
}

pub(crate) fn check_spelling(text: &str, limits: &Limits) -> Result<(), NumberError> {
    inspect(text, limits, false).map(|_| ())
}

impl DecimalLiteral {
    pub fn from_info(info: &DecimalInfo, work: &mut WorkBudget) -> Result<Self, NumberError> {
        work.charge(info.coefficient().len() as u32)
            .map_err(|_| NumberError::Work)?;
        let value =
            BigInt::parse_bytes(info.coefficient().as_bytes(), 10).ok_or(NumberError::Form)?;
        Ok(Self {
            info: info.clone(),
            value,
        })
    }

    pub fn parse(text: &str, limits: &Limits, work: &mut WorkBudget) -> Result<Self, NumberError> {
        Self::parse_decimal(text, limits, work, false)
    }

    pub fn parse_formatted(
        text: &str,
        limits: &Limits,
        work: &mut WorkBudget,
    ) -> Result<Self, NumberError> {
        Self::parse_decimal(text, limits, work, true)
    }

    fn parse_decimal(
        text: &str,
        limits: &Limits,
        work: &mut WorkBudget,
        formatted: bool,
    ) -> Result<Self, NumberError> {
        let spelling = inspect(text, limits, formatted)?;
        work.charge(spelling.digits)
            .map_err(|_| NumberError::Work)?;
        let Spelling {
            negative,
            integer,
            fraction,
            exponent,
            scaled,
            ..
        } = spelling;
        let fractional_digits =
            u32::try_from(fraction.map_or(0, str::len)).map_err(|_| NumberError::Limit)?;
        let mut coefficient = String::with_capacity(text.len());
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
                scaled,
                computed: false,
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

    /// Compares this bound after rounding to the base bound's declared scale.
    pub fn compare_rounded_to(
        &self,
        base: &Self,
        work: &mut WorkBudget,
    ) -> Result<Ordering, NumberError> {
        let scale = self.info.scale();
        let base_scale = base.info.scale();
        if scale <= base_scale {
            return self.compare_value(base, work);
        }
        let distance = scale.abs_diff(base_scale);
        work.charge(distance).map_err(|_| NumberError::Work)?;
        let divisor = BigInt::from(10_u8).pow(distance);
        let mut rounded = &self.value / &divisor;
        let remainder = &self.value % &divisor;
        if remainder.magnitude() * 2_u8 >= *divisor.magnitude() {
            if self.value.sign() == Sign::Minus {
                rounded -= 1;
            } else {
                rounded += 1;
            }
        }
        Ok(rounded.cmp(&base.value))
    }
}

const GRID: i64 = 1024;
const LIMIT: i64 = 2048 * GRID;

struct ArithmeticInterval {
    lower: i64,
    upper: i64,
    integral: bool,
}

/// Computes only ranges whose operands and results are exact across the
/// IEEE-754 formats covered by the model's arithmetic profile.
pub(crate) fn arithmetic_bounds(
    operator: BinaryOperator,
    left: (&DecimalInfo, &DecimalInfo),
    right: (&DecimalInfo, &DecimalInfo),
    work: &mut WorkBudget,
) -> Result<Option<NumericBounds>, NumberError> {
    let Some(x) = arithmetic_interval(left, work)? else {
        return Ok(None);
    };
    let Some(y) = arithmetic_interval(right, work)? else {
        return Ok(None);
    };
    let singleton = x.lower == x.upper && y.lower == y.upper;
    let exact_division = y.lower == y.upper && (y.lower / GRID).unsigned_abs().is_power_of_two();
    if !singleton
        && (!x.integral || !y.integral || operator == BinaryOperator::Divide && !exact_division)
    {
        return Ok(None);
    }
    let mut minimum = i64::MAX;
    let mut maximum = i64::MIN;
    for a in [x.lower, x.upper] {
        for b in [y.lower, y.upper] {
            let (numerator, denominator) = match operator {
                BinaryOperator::Add => (a + b, 1),
                BinaryOperator::Subtract => (a - b, 1),
                BinaryOperator::Multiply => (a * b, GRID),
                BinaryOperator::Divide => (a * GRID, b),
                _ => unreachable!("only arithmetic operators have numeric bounds"),
            };
            if denominator == 0 || numerator % denominator != 0 {
                return Ok(None);
            }
            let value = numerator / denominator;
            if !exact_grid_value(value) {
                return Ok(None);
            }
            minimum = minimum.min(value);
            maximum = maximum.max(value);
        }
    }
    Ok(Some(NumericBounds {
        lower: grid_decimal(minimum),
        upper: grid_decimal(maximum),
    }))
}

fn arithmetic_interval(
    (low, high): (&DecimalInfo, &DecimalInfo),
    work: &mut WorkBudget,
) -> Result<Option<ArithmeticInterval>, NumberError> {
    work.charge((low.coefficient().len() + high.coefficient().len()) as u32)
        .map_err(|_| NumberError::Work)?;
    let (Some(a), Some(b)) = (on_grid(low), on_grid(high)) else {
        return Ok(None);
    };
    Ok(Some(ArithmeticInterval {
        lower: a,
        upper: b,
        integral: a % GRID == 0
            && b % GRID == 0
            && (a == b || low.scale() <= 0 && high.scale() <= 0),
    }))
}

fn on_grid(value: &DecimalInfo) -> Option<i64> {
    let scaled = value
        .coefficient()
        .parse::<i128>()
        .ok()?
        .checked_mul(i128::from(GRID))?;
    let power = 10_i128.checked_pow(value.scale().unsigned_abs())?;
    let number = if value.scale() < 0 {
        scaled.checked_mul(power)?
    } else {
        if scaled % power != 0 {
            return None;
        }
        scaled / power
    };
    let grid = i64::try_from(number).ok()?;
    exact_grid_value(grid).then_some(grid)
}

fn exact_grid_value(value: i64) -> bool {
    let magnitude = value.unsigned_abs();
    magnitude <= LIMIT as u64
        && (magnitude == 0
            || magnitude.ilog2() + 1 - magnitude.trailing_zeros() <= 11
                && grid_coefficient(magnitude).0 <= 9_999_999)
}

fn grid_coefficient(magnitude: u64) -> (u64, u32) {
    // 10^10 / 1024. The grid bound keeps this product within u64.
    let mut coefficient = magnitude * 9_765_625;
    let mut scale = 10;
    while scale > 0 && coefficient.is_multiple_of(10) {
        coefficient /= 10;
        scale -= 1;
    }
    (coefficient, scale)
}

fn grid_decimal(value: i64) -> DecimalInfo {
    let (coefficient, scale) = grid_coefficient(value.unsigned_abs());
    let sign = if value < 0 { "-" } else { "" };
    let original = if scale == 0 {
        format!("{sign}{coefficient}")
    } else {
        let power = 10_u64.pow(scale);
        format!(
            "{sign}{}.{:0width$}",
            coefficient / power,
            coefficient % power,
            width = scale as usize
        )
    };
    DecimalInfo {
        original,
        coefficient: format!("{sign}{coefficient}"),
        fractional_digits: scale,
        exponent: 0,
        scaled: false,
        computed: true,
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
