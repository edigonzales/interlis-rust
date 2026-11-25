use crate::{
    BinaryOperator, Limits, NumericBounds, UnitConstant, limits::WorkBudget, model::DecimalInfo,
};
use num_bigint::{BigInt, Sign};
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NumberError {
    Form,
    Limit,
    Work,
}
#[derive(Clone)]
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

pub(crate) fn constant_info(value: &UnitConstant) -> &DecimalInfo {
    use std::sync::LazyLock;
    fn predefined(text: &str) -> DecimalInfo {
        let (_, fraction) = text.split_once('.').expect("predefined decimal");
        DecimalInfo {
            original: text.into(),
            coefficient: text.replace('.', ""),
            fractional_digits: fraction.len() as u32,
            exponent: 0,
            scaled: false,
            computed: true,
        }
    }
    static PI: LazyLock<DecimalInfo> =
        LazyLock::new(|| predefined("3.1415926535897932384626433832795028841971693993751"));
    static LNBASE: LazyLock<DecimalInfo> =
        LazyLock::new(|| predefined("2.7182818284590452353602874713526624977572470937"));
    match value {
        UnitConstant::Decimal(value) => value,
        UnitConstant::Pi => &PI,
        UnitConstant::LnBase => &LNBASE,
    }
}

const DIVISION_PRECISION: i32 = 50;

#[derive(Clone, Copy)]
pub(crate) enum DivisionRounding {
    Nearest,
    Lower,
    Upper,
}

impl DecimalLiteral {
    fn computed(
        value: BigInt,
        scale: i32,
        limits: &Limits,
        work: &mut WorkBudget,
    ) -> Result<Self, NumberError> {
        let coefficient = value.to_string();
        let (_, digits) = coefficient.split_at(usize::from(value.sign() == Sign::Minus));
        let trim = if value.sign() == Sign::NoSign {
            scale.max(0) as usize
        } else {
            (scale.max(0) as usize).min(digits.len() - digits.trim_end_matches('0').len())
        };
        let scale = if value.sign() == Sign::NoSign {
            0
        } else {
            scale - trim as i32
        };
        let coefficient = if value.sign() == Sign::NoSign {
            "0".to_owned()
        } else {
            coefficient[..coefficient.len() - trim].to_owned()
        };
        let digits = coefficient.trim_start_matches('-');
        if digits.len() > limits.max_numeric_digits as usize
            || scale.unsigned_abs() > limits.max_abs_exponent
        {
            return Err(NumberError::Limit);
        }
        charge_decimal_work(work, coefficient.len() as u64)?;
        let original = if scale == 0 {
            coefficient.clone()
        } else if (1..=256).contains(&scale) {
            let sign = if value.sign() == Sign::Minus { "-" } else { "" };
            let scale = scale as usize;
            if digits.len() <= scale {
                format!("{sign}0.{}{digits}", "0".repeat(scale - digits.len()))
            } else {
                let (integer, fraction) = digits.split_at(digits.len() - scale);
                format!("{sign}{integer}.{fraction}")
            }
        } else {
            format!("{coefficient}e{}", -scale)
        };
        let value = BigInt::parse_bytes(coefficient.as_bytes(), 10).ok_or(NumberError::Form)?;
        Ok(Self {
            info: DecimalInfo {
                original,
                coefficient,
                fractional_digits: if (1..=256).contains(&scale) {
                    scale as u32
                } else {
                    0
                },
                exponent: if (0..=256).contains(&scale) {
                    0
                } else {
                    -scale
                },
                scaled: !(0..=256).contains(&scale),
                computed: true,
            },
            value,
        })
    }

    pub(crate) fn operate(
        &self,
        operator: BinaryOperator,
        other: &Self,
        rounding: DivisionRounding,
        limits: &Limits,
        work: &mut WorkBudget,
    ) -> Result<Self, NumberError> {
        use BinaryOperator::{Add, Divide, Multiply, Subtract};
        let a = self.info.scale();
        let b = other.info.scale();
        let size = self
            .info
            .coefficient
            .len()
            .max(other.info.coefficient.len()) as u64;
        charge_decimal_work(work, size)?;
        let (value, scale) = match operator {
            Add | Subtract => {
                let scale = a.max(b);
                let av = align_decimal(&self.value, (scale - a) as u32, work)?;
                let bv = align_decimal(&other.value, (scale - b) as u32, work)?;
                (if operator == Add { av + bv } else { av - bv }, scale)
            }
            Multiply => {
                charge_decimal_work(work, size.saturating_mul(size).div_ceil(32))?;
                (&self.value * &other.value, a + b)
            }
            Divide => {
                return divide_decimal(&self.value, &other.value, a - b, rounding, limits, work);
            }
            _ => unreachable!("only arithmetic operators have numeric bounds"),
        };
        Self::computed(value, scale, limits, work)
    }
}

fn charge_decimal_work(work: &mut WorkBudget, cost: u64) -> Result<(), NumberError> {
    work.charge(u32::try_from(cost).map_err(|_| NumberError::Limit)?)
        .map_err(|_| NumberError::Work)
}

fn align_decimal(
    value: &BigInt,
    places: u32,
    work: &mut WorkBudget,
) -> Result<BigInt, NumberError> {
    charge_decimal_work(work, u64::from(places))?;
    Ok(if places == 0 || value.sign() == Sign::NoSign {
        value.clone()
    } else {
        value * BigInt::from(10_u8).pow(places)
    })
}

fn terminating_fraction(
    numerator: &BigInt,
    denominator: &BigInt,
    work: &mut WorkBudget,
) -> Result<Option<(BigInt, u32)>, NumberError> {
    let mut common = numerator.magnitude().clone();
    let mut remainder = denominator.magnitude().clone();
    while remainder.bits() != 0 {
        charge_decimal_work(work, common.bits().max(remainder.bits()).div_ceil(8))?;
        let next = &common % &remainder;
        common = remainder;
        remainder = next;
    }
    let mut reduced = denominator.magnitude() / &common;
    let twos = reduced.trailing_zeros().expect("nonzero denominator") as u32;
    reduced >>= twos;
    let mut fives = 0;
    while &reduced % 5_u8 == 0_u8.into() {
        charge_decimal_work(work, reduced.bits().div_ceil(8))?;
        reduced /= 5_u8;
        fives += 1;
    }
    if reduced != 1_u8.into() {
        return Ok(None);
    }
    let places = twos.max(fives);
    charge_decimal_work(work, u64::from(places))?;
    let value = (numerator / BigInt::from(common))
        * (BigInt::from(2_u8).pow(places - twos) * BigInt::from(5_u8).pow(places - fives));
    Ok(Some((value, places)))
}

fn divide_decimal(
    numerator: &BigInt,
    denominator: &BigInt,
    scale: i32,
    rounding: DivisionRounding,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<DecimalLiteral, NumberError> {
    debug_assert_ne!(denominator.sign(), Sign::NoSign);
    if numerator.sign() == Sign::NoSign {
        return DecimalLiteral::computed(BigInt::from(0), 0, limits, work);
    }
    let negative = numerator.sign() != denominator.sign();
    if matches!(rounding, DivisionRounding::Nearest)
        && let Some((value, places)) = terminating_fraction(numerator, denominator, work)?
    {
        let value = if denominator.sign() == Sign::Minus {
            -value
        } else {
            value
        };
        return DecimalLiteral::computed(value, scale + places as i32, limits, work);
    }
    let numerator = BigInt::from(numerator.magnitude().clone());
    let denominator = BigInt::from(denominator.magnitude().clone());
    let mut order = numerator.to_string().len() as i32 - denominator.to_string().len() as i32;
    let below_power = if order >= 0 {
        numerator < align_decimal(&denominator, order as u32, work)?
    } else {
        align_decimal(&numerator, order.unsigned_abs(), work)? < denominator
    };
    if below_power {
        order -= 1;
    }
    let places = DIVISION_PRECISION - 1 - order;
    let (numerator, denominator) = if places >= 0 {
        (align_decimal(&numerator, places as u32, work)?, denominator)
    } else {
        (
            numerator,
            align_decimal(&denominator, places.unsigned_abs(), work)?,
        )
    };
    charge_decimal_work(work, numerator.bits().max(denominator.bits()).div_ceil(8))?;
    let mut value = &numerator / &denominator;
    let remainder = numerator % &denominator;
    let round_up = remainder.sign() != Sign::NoSign
        && match rounding {
            DivisionRounding::Lower => negative,
            DivisionRounding::Upper => !negative,
            DivisionRounding::Nearest => match (&remainder * 2_u8).cmp(&denominator) {
                Ordering::Greater => true,
                Ordering::Equal => &value % 2_u8 != BigInt::from(0),
                Ordering::Less => false,
            },
        };
    if round_up {
        value += 1;
    }
    if negative {
        value = -value;
    }
    DecimalLiteral::computed(value, scale + places, limits, work)
}

/// Derives a numeric enclosure using the same decimal profile as evaluation.
/// Division of an interval rounds outwards when some quotients may be rounded.
pub(crate) fn arithmetic_bounds(
    operator: BinaryOperator,
    left: (&DecimalInfo, &DecimalInfo),
    right: (&DecimalInfo, &DecimalInfo),
    same_operand: bool,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<Option<(NumericBounds, bool)>, NumberError> {
    let a = [
        DecimalLiteral::from_info(left.0, work)?,
        DecimalLiteral::from_info(left.1, work)?,
    ];
    let b = [
        DecimalLiteral::from_info(right.0, work)?,
        DecimalLiteral::from_info(right.1, work)?,
    ];
    let divide = operator == BinaryOperator::Divide;
    if divide && (b[0].value.sign() != b[1].value.sign() || b[0].value.sign() == Sign::NoSign) {
        return Ok(None);
    }
    let exact = !divide
        || b[0].compare_value(&b[1], work)? == Ordering::Equal
            && (a[0].compare_value(&a[1], work)? == Ordering::Equal
                || terminating_fraction(&BigInt::from(1), &b[0].value, work)?.is_some());
    let mut rounded = false;
    let mut minimum: Option<DecimalLiteral> = None;
    let mut maximum: Option<DecimalLiteral> = None;
    for (i, left) in a.iter().enumerate() {
        for (j, right) in b.iter().enumerate() {
            if same_operand && i != j {
                continue;
            }
            let lower = left.operate(
                operator,
                right,
                if exact {
                    DivisionRounding::Nearest
                } else {
                    DivisionRounding::Lower
                },
                limits,
                work,
            )?;
            let upper = if exact {
                lower.clone()
            } else {
                left.operate(operator, right, DivisionRounding::Upper, limits, work)?
            };
            rounded |= lower.compare_value(&upper, work)? != Ordering::Equal;
            if minimum
                .as_ref()
                .map(|value| lower.compare_value(value, work))
                .transpose()?
                != Some(Ordering::Greater)
            {
                minimum = Some(lower);
            }
            if maximum
                .as_ref()
                .map(|value| upper.compare_value(value, work))
                .transpose()?
                != Some(Ordering::Less)
            {
                maximum = Some(upper);
            }
        }
    }
    if same_operand
        && operator == BinaryOperator::Multiply
        && a[0].value.sign() != Sign::Plus
        && a[1].value.sign() != Sign::Minus
    {
        minimum = Some(DecimalLiteral::computed(BigInt::from(0), 0, limits, work)?);
    }
    Ok(Some((
        NumericBounds {
            lower: minimum.expect("arithmetic has endpoints").info,
            upper: maximum.expect("arithmetic has endpoints").info,
        },
        rounded,
    )))
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
