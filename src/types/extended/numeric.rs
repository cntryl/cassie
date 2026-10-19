//! `numeric` / `decimal`: PostgreSQL's arbitrary-precision exact decimal.
//!
//! A finite value is `mantissa * 10^-scale` where `scale` is PostgreSQL's
//! display scale (`dscale`, never negative). Text input, output, typmod
//! coercion, ordering and the base-10000 binary frame follow `numeric.c`.
use std::cmp::Ordering;

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::TypeInputError;

#[path = "numeric_math.rs"]
pub mod math;

/// `NUMERIC_MAX_PRECISION`.
pub const MAX_PRECISION: i32 = 1000;
/// `NUMERIC_MAX_DISPLAY_SCALE`.
pub const MAX_DISPLAY_SCALE: u32 = 1000;
/// `NUMERIC_DSCALE_MAX`.
pub const MAX_DSCALE: u32 = 0x3FFF;
/// Decimal digits before the point allowed by `NUMERIC_WEIGHT_MAX`.
pub const MAX_INTEGER_DIGITS: usize = 131_072;

const SIGN_POSITIVE: u16 = 0x0000;
const SIGN_NEGATIVE: u16 = 0x4000;
const SIGN_NAN: u16 = 0xC000;
const SIGN_POSITIVE_INFINITY: u16 = 0xD000;
const SIGN_NEGATIVE_INFINITY: u16 = 0xF000;

/// A declared `numeric(precision, scale)` modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NumericTypmod {
    pub precision: u16,
    pub scale: i16,
}

impl NumericTypmod {
    /// Validates a declared modifier with PostgreSQL's messages.
    ///
    /// # Errors
    /// Returns `22023` when the precision or scale is outside PostgreSQL's range.
    pub fn new(precision: i64, scale: i64) -> Result<Self, TypeInputError> {
        if !(1..=i64::from(MAX_PRECISION)).contains(&precision) {
            return Err(TypeInputError {
                sqlstate: "22023",
                message: format!("NUMERIC precision {precision} must be between 1 and 1000"),
            });
        }
        if !(-1000..=1000).contains(&scale) {
            return Err(TypeInputError {
                sqlstate: "22023",
                message: format!("NUMERIC scale {scale} must be between -1000 and 1000"),
            });
        }
        Ok(Self {
            precision: u16::try_from(precision).unwrap_or(1),
            scale: i16::try_from(scale).unwrap_or_default(),
        })
    }

    /// The `atttypmod` PostgreSQL reports for this modifier.
    #[must_use]
    pub fn atttypmod(self) -> i32 {
        ((i32::from(self.precision) << 16) | (i32::from(self.scale) & 0x7ff)) + 4
    }

    /// Decodes an `atttypmod` value; `-1` and malformed values are unconstrained.
    #[must_use]
    pub fn from_atttypmod(typmod: i32) -> Option<Self> {
        if typmod < 4 {
            return None;
        }
        let raw = typmod - 4;
        let precision = (raw >> 16) & 0xffff;
        // The scale is an 11-bit two's-complement field.
        let scale = ((raw & 0x7ff) ^ 0x400) - 0x400;
        Self::new(i64::from(precision), i64::from(scale)).ok()
    }
}

/// A PostgreSQL `numeric` datum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Numeric {
    NaN,
    Infinity,
    NegInfinity,
    Finite { mantissa: BigInt, scale: u32 },
}

impl Serialize for Numeric {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Numeric {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_text(&text).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Numeric {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NaN => formatter.write_str("NaN"),
            Self::Infinity => formatter.write_str("Infinity"),
            Self::NegInfinity => formatter.write_str("-Infinity"),
            Self::Finite { mantissa, scale } => {
                formatter.write_str(&format_fixed(mantissa, *scale))
            }
        }
    }
}

/// Renders `mantissa * 10^-scale` with exactly `scale` fractional digits.
fn format_fixed(mantissa: &BigInt, scale: u32) -> String {
    let digits = mantissa.magnitude().to_string();
    let scale = scale as usize;
    let mut output = String::with_capacity(digits.len() + scale + 3);
    if mantissa.is_negative() {
        output.push('-');
    }
    if scale == 0 {
        output.push_str(&digits);
        return output;
    }
    if digits.len() <= scale {
        output.push_str("0.");
        output.extend(std::iter::repeat_n('0', scale - digits.len()));
        output.push_str(&digits);
    } else {
        let (integer, fraction) = digits.split_at(digits.len() - scale);
        output.push_str(integer);
        output.push('.');
        output.push_str(fraction);
    }
    output
}

/// `10^exponent` as a big integer.
#[must_use]
pub fn pow10(exponent: u32) -> BigInt {
    num_traits::pow(BigInt::from(10u8), exponent as usize)
}

impl Numeric {
    #[must_use]
    pub fn zero() -> Self {
        Self::Finite {
            mantissa: BigInt::zero(),
            scale: 0,
        }
    }

    #[must_use]
    pub fn from_i64(value: i64) -> Self {
        Self::Finite {
            mantissa: BigInt::from(value),
            scale: 0,
        }
    }

    #[must_use]
    pub fn from_i128(value: i128) -> Self {
        Self::Finite {
            mantissa: BigInt::from(value),
            scale: 0,
        }
    }

    #[must_use]
    pub fn finite(mantissa: BigInt, scale: u32) -> Self {
        Self::Finite { mantissa, scale }
    }

    #[must_use]
    pub const fn is_nan(&self) -> bool {
        matches!(self, Self::NaN)
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        matches!(self, Self::Finite { mantissa, .. } if mantissa.is_zero())
    }

    /// The display scale (`scale()` returns NULL for specials).
    #[must_use]
    pub const fn scale(&self) -> Option<u32> {
        match self {
            Self::Finite { scale, .. } => Some(*scale),
            _ => None,
        }
    }

    /// -1, 0 or 1 by sign; `None` for NaN.
    #[must_use]
    pub fn signum(&self) -> Option<i8> {
        match self {
            Self::NaN => None,
            Self::Infinity => Some(1),
            Self::NegInfinity => Some(-1),
            Self::Finite { mantissa, .. } => Some(match mantissa.sign() {
                Sign::Minus => -1,
                Sign::NoSign => 0,
                Sign::Plus => 1,
            }),
        }
    }

    /// The same value expressed with display scale `scale` (only widening).
    #[must_use]
    pub fn rescaled_mantissa(mantissa: &BigInt, from: u32, to: u32) -> BigInt {
        if to >= from {
            mantissa * pow10(to - from)
        } else {
            mantissa / pow10(from - to)
        }
    }

    /// Rounds half away from zero to `scale` digits after the point
    /// (negative scales round to the left of it), `round_var`.
    #[must_use]
    pub fn round_to(&self, scale: i32) -> Self {
        let Self::Finite {
            mantissa,
            scale: current,
        } = self
        else {
            return self.clone();
        };
        let current_i = i64::from(*current);
        let target = i64::from(scale);
        let display = u32::try_from(scale.max(0)).unwrap_or_default();
        if target >= current_i {
            return Self::finite(
                Self::rescaled_mantissa(mantissa, *current, display),
                display,
            );
        }
        let drop = u32::try_from(current_i - target).unwrap_or(u32::MAX);
        let divisor = pow10(drop);
        let (mut quotient, remainder) = mantissa.div_rem(&divisor);
        if remainder.magnitude() * 2u8 >= *divisor.magnitude() {
            if mantissa.is_negative() {
                quotient -= 1;
            } else {
                quotient += 1;
            }
        }
        let mantissa = if target < 0 {
            quotient * pow10(u32::try_from(-target).unwrap_or_default())
        } else {
            quotient
        };
        Self::finite(mantissa, display)
    }

    /// Truncates toward zero to `scale` digits, `trunc_var`.
    #[must_use]
    pub fn truncate_to(&self, scale: i32) -> Self {
        let Self::Finite {
            mantissa,
            scale: current,
        } = self
        else {
            return self.clone();
        };
        let current_i = i64::from(*current);
        let target = i64::from(scale);
        let display = u32::try_from(scale.max(0)).unwrap_or_default();
        if target >= current_i {
            return Self::finite(
                Self::rescaled_mantissa(mantissa, *current, display),
                display,
            );
        }
        let drop = u32::try_from(current_i - target).unwrap_or(u32::MAX);
        let quotient = mantissa / pow10(drop);
        let mantissa = if target < 0 {
            quotient * pow10(u32::try_from(-target).unwrap_or_default())
        } else {
            quotient
        };
        Self::finite(mantissa, display)
    }

    /// Decimal digits before the point of a finite nonzero value.
    #[must_use]
    pub fn integer_digits(&self) -> usize {
        match self {
            Self::Finite { mantissa, scale } if !mantissa.is_zero() => {
                let digits = mantissa.magnitude().to_string().len();
                digits.saturating_sub(*scale as usize)
            }
            _ => 0,
        }
    }

    /// The position of the leading digit relative to the decimal point
    /// (`1` for `5`, `-2` for `0.001`); `None` for zero and specials.
    #[must_use]
    pub fn decimal_weight(&self) -> Option<i64> {
        match self {
            Self::Finite { mantissa, scale } if !mantissa.is_zero() => {
                let digits = i64::try_from(mantissa.magnitude().to_string().len()).ok()?;
                Some(digits - i64::from(*scale))
            }
            _ => None,
        }
    }

    /// Rejects values whose weight or display scale exceed the format.
    ///
    /// # Errors
    /// Returns `22003` "value overflows numeric format".
    pub fn check_format(self) -> Result<Self, TypeInputError> {
        if let Self::Finite { scale, .. } = &self {
            if *scale > MAX_DSCALE || self.integer_digits() > MAX_INTEGER_DIGITS {
                return Err(overflow());
            }
        }
        Ok(self)
    }

    /// `apply_typmod`: rounds to the declared scale and checks precision.
    ///
    /// # Errors
    /// Returns `22003` "numeric field overflow" when the value does not fit.
    pub fn apply_typmod(self, typmod: Option<NumericTypmod>) -> Result<Self, TypeInputError> {
        let Some(typmod) = typmod else {
            return Ok(self);
        };
        match self {
            Self::NaN => Ok(Self::NaN),
            Self::Infinity | Self::NegInfinity => Err(TypeInputError {
                sqlstate: "22003",
                message: "numeric field overflow".to_string(),
            }),
            finite @ Self::Finite { .. } => {
                let rounded = finite.round_to(i32::from(typmod.scale));
                let max_digits = i64::from(typmod.precision) - i64::from(typmod.scale);
                if rounded
                    .decimal_weight()
                    .is_some_and(|digits| digits > max_digits)
                {
                    return Err(TypeInputError {
                        sqlstate: "22003",
                        message: "numeric field overflow".to_string(),
                    });
                }
                Ok(rounded)
            }
        }
    }

    /// Compares by value with PostgreSQL's order (NaN last, equal to itself).
    #[must_use]
    pub fn compare(&self, other: &Self) -> Ordering {
        const fn class(value: &Numeric) -> u8 {
            match value {
                Numeric::NegInfinity => 0,
                Numeric::Finite { .. } => 1,
                Numeric::Infinity => 2,
                Numeric::NaN => 3,
            }
        }
        match (self, other) {
            (
                Self::Finite {
                    mantissa: left,
                    scale: left_scale,
                },
                Self::Finite {
                    mantissa: right,
                    scale: right_scale,
                },
            ) => {
                let scale = (*left_scale).max(*right_scale);
                Self::rescaled_mantissa(left, *left_scale, scale).cmp(&Self::rescaled_mantissa(
                    right,
                    *right_scale,
                    scale,
                ))
            }
            _ => class(self).cmp(&class(other)),
        }
    }

    /// A canonical key: equal values (`1.0`, `1.00`) produce equal keys.
    #[must_use]
    pub fn order_key(&self) -> NumericKey {
        match self {
            Self::Finite { mantissa, scale } => {
                let mut mantissa = mantissa.clone();
                let mut exponent = -i64::from(*scale);
                if mantissa.is_zero() {
                    exponent = 0;
                } else {
                    let ten = BigInt::from(10u8);
                    loop {
                        let (quotient, remainder) = mantissa.div_rem(&ten);
                        if !remainder.is_zero() {
                            break;
                        }
                        mantissa = quotient;
                        exponent += 1;
                    }
                }
                NumericKey::Finite { mantissa, exponent }
            }
            Self::NaN => NumericKey::NaN,
            Self::Infinity => NumericKey::Infinity,
            Self::NegInfinity => NumericKey::NegInfinity,
        }
    }

    /// `numeric_float8`: the nearest double.
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        match self {
            Self::NaN => f64::NAN,
            Self::Infinity => f64::INFINITY,
            Self::NegInfinity => f64::NEG_INFINITY,
            Self::Finite { .. } => self.to_string().parse::<f64>().unwrap_or(f64::NAN),
        }
    }

    /// `float8_numeric`: the double's `DBL_DIG` (15) significant digits.
    ///
    /// # Errors
    /// Never fails for finite input; specials map to numeric specials.
    pub fn from_f64(value: f64) -> Result<Self, TypeInputError> {
        Self::from_float_digits(value, 15)
    }

    /// `float4_numeric`: the real's `FLT_DIG` (6) significant digits.
    ///
    /// # Errors
    /// Never fails for finite input; specials map to numeric specials.
    pub fn from_f32(value: f32) -> Result<Self, TypeInputError> {
        Self::from_float_digits(f64::from(value), 6)
    }

    fn from_float_digits(value: f64, digits: usize) -> Result<Self, TypeInputError> {
        if value.is_nan() {
            return Ok(Self::NaN);
        }
        if value.is_infinite() {
            return Ok(if value > 0.0 {
                Self::Infinity
            } else {
                Self::NegInfinity
            });
        }
        // `%.*g` keeps `digits` significant digits; trailing zeros of the
        // mantissa are dropped as C's %g does.
        let formatted = format!("{:.*e}", digits.saturating_sub(1), value);
        let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.trim_end_matches('0').trim_end_matches('.')
        } else {
            mantissa
        };
        parse_text(&format!("{mantissa}e{exponent}"))
    }

    /// Converts to `i64` with PostgreSQL's rounding (half away from zero).
    ///
    /// # Errors
    /// Returns PostgreSQL's NaN/infinity messages, or `None` when the rounded
    /// value is outside the `i64` range.
    pub fn to_i64_rounded(&self) -> Result<Option<i64>, TypeInputError> {
        match self {
            Self::NaN => Err(TypeInputError {
                sqlstate: "0A000",
                message: "cannot convert NaN to integer".to_string(),
            }),
            Self::Infinity | Self::NegInfinity => Err(TypeInputError {
                sqlstate: "0A000",
                message: "cannot convert infinity to integer".to_string(),
            }),
            Self::Finite { .. } => {
                let Self::Finite { mantissa, .. } = self.round_to(0) else {
                    return Ok(None);
                };
                Ok(mantissa.to_i64())
            }
        }
    }

    /// The base-10000 digits and weight of a finite value (`NumericVar`).
    #[must_use]
    pub fn base_digits(mantissa: &BigInt, scale: u32) -> (Vec<i16>, i32) {
        let digits = mantissa.magnitude().to_string();
        let scale = scale as usize;
        let padded = if digits.len() <= scale {
            format!("{}{digits}", "0".repeat(scale + 1 - digits.len()))
        } else {
            digits
        };
        let (integer, fraction) = padded.split_at(padded.len() - scale);
        let integer_pad = (4 - integer.len() % 4) % 4;
        let fraction_pad = (4 - fraction.len() % 4) % 4;
        let integer = format!("{}{integer}", "0".repeat(integer_pad));
        let fraction = format!("{fraction}{}", "0".repeat(fraction_pad));
        let mut groups: Vec<i16> = integer
            .as_bytes()
            .chunks(4)
            .chain(fraction.as_bytes().chunks(4))
            .map(|chunk| {
                chunk
                    .iter()
                    .fold(0i16, |value, digit| value * 10 + i16::from(digit - b'0'))
            })
            .collect();
        let mut weight = i32::try_from(integer.len() / 4).unwrap_or(i32::MAX) - 1;
        let leading = groups.iter().take_while(|group| **group == 0).count();
        groups.drain(..leading);
        weight -= i32::try_from(leading).unwrap_or_default();
        while groups.last() == Some(&0) {
            groups.pop();
        }
        if groups.is_empty() {
            weight = 0;
        }
        (groups, weight)
    }

    /// `numeric_send`.
    #[must_use]
    pub fn send_binary(&self) -> Vec<u8> {
        let (digits, weight, sign, scale) = match self {
            Self::NaN => (Vec::new(), 0, SIGN_NAN, 0),
            // PostgreSQL 18 sends infinities with display scale 32.
            Self::Infinity => (Vec::new(), 0, SIGN_POSITIVE_INFINITY, 32),
            Self::NegInfinity => (Vec::new(), 0, SIGN_NEGATIVE_INFINITY, 32),
            Self::Finite { mantissa, scale } => {
                let (digits, weight) = Self::base_digits(mantissa, *scale);
                let sign = if mantissa.is_negative() {
                    SIGN_NEGATIVE
                } else {
                    SIGN_POSITIVE
                };
                (digits, weight, sign, *scale)
            }
        };
        let mut bytes = Vec::with_capacity(8 + digits.len() * 2);
        bytes.extend_from_slice(
            &u16::try_from(digits.len())
                .unwrap_or_default()
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&i16::try_from(weight).unwrap_or_default().to_be_bytes());
        bytes.extend_from_slice(&sign.to_be_bytes());
        bytes.extend_from_slice(&u16::try_from(scale).unwrap_or_default().to_be_bytes());
        for digit in digits {
            bytes.extend_from_slice(&digit.to_be_bytes());
        }
        bytes
    }
}

/// A totally ordered, hashable form of a numeric value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NumericKey {
    NegInfinity,
    Finite { mantissa: BigInt, exponent: i64 },
    Infinity,
    NaN,
}

impl NumericKey {
    const fn class(&self) -> u8 {
        match self {
            Self::NegInfinity => 0,
            Self::Finite { .. } => 1,
            Self::Infinity => 2,
            Self::NaN => 3,
        }
    }
}

impl PartialOrd for NumericKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NumericKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let (
            Self::Finite {
                mantissa: left,
                exponent: left_exponent,
            },
            Self::Finite {
                mantissa: right,
                exponent: right_exponent,
            },
        ) = (self, other)
        else {
            return self.class().cmp(&other.class());
        };
        let sign = left.sign().cmp(&right.sign());
        if sign != Ordering::Equal || left.is_zero() {
            return sign;
        }
        let magnitude = |mantissa: &BigInt, exponent: i64| {
            i64::try_from(mantissa.magnitude().to_string().len()).unwrap_or(i64::MAX) + exponent
        };
        let by_magnitude = magnitude(left, *left_exponent).cmp(&magnitude(right, *right_exponent));
        let ordering = if by_magnitude == Ordering::Equal {
            let base = (*left_exponent).min(*right_exponent);
            let shift = |exponent: i64| pow10(u32::try_from(exponent - base).unwrap_or_default());
            (left.magnitude() * shift(*left_exponent).magnitude())
                .cmp(&(right.magnitude() * shift(*right_exponent).magnitude()))
        } else {
            by_magnitude
        };
        if left.is_negative() {
            ordering.reverse()
        } else {
            ordering
        }
    }
}

fn overflow() -> TypeInputError {
    TypeInputError {
        sqlstate: "22003",
        message: "value overflows numeric format".to_string(),
    }
}

fn syntax(text: &str) -> TypeInputError {
    TypeInputError {
        sqlstate: "22P02",
        message: format!("invalid input syntax for type numeric: \"{text}\""),
    }
}

/// Strips `_` separators that sit between two digits of `radix`.
fn strip_separators(digits: &str, radix: u32) -> Option<String> {
    let bytes = digits.as_bytes();
    let mut output = String::with_capacity(digits.len());
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'_' {
            let before = index
                .checked_sub(1)
                .and_then(|previous| bytes.get(previous))
                .is_some_and(|digit| char::from(*digit).is_digit(radix));
            let after = bytes
                .get(index + 1)
                .is_some_and(|digit| char::from(*digit).is_digit(radix));
            if !before || !after {
                return None;
            }
        } else {
            output.push(char::from(*byte));
        }
    }
    Some(output)
}

/// `numeric_in`.
///
/// # Errors
/// Returns `22P02` for malformed text and `22003` when the value overflows
/// the numeric format.
pub fn parse_text(text: &str) -> Result<Numeric, TypeInputError> {
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\u{b}');
    if trimmed.eq_ignore_ascii_case("nan") {
        return Ok(Numeric::NaN);
    }
    let negative = trimmed.starts_with('-');
    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if body.eq_ignore_ascii_case("infinity") || body.eq_ignore_ascii_case("inf") {
        return Ok(if negative {
            Numeric::NegInfinity
        } else {
            Numeric::Infinity
        });
    }
    let radix = match body.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("0x") => Some(16),
        Some("0o") => Some(8),
        Some("0b") => Some(2),
        _ => None,
    };
    if let Some(radix) = radix {
        let digits = body[2..].strip_prefix('_').unwrap_or(&body[2..]);
        let digits = strip_separators(digits, radix).ok_or_else(|| syntax(text))?;
        if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
            return Err(syntax(text));
        }
        let magnitude =
            BigInt::parse_bytes(digits.as_bytes(), radix).ok_or_else(|| syntax(text))?;
        let mantissa = if negative { -magnitude } else { magnitude };
        return Numeric::finite(mantissa, 0).check_format();
    }
    let (number, exponent) = match body.find(['e', 'E']) {
        Some(position) => (&body[..position], Some(&body[position + 1..])),
        None => (body, None),
    };
    let number = strip_separators(number, 10).ok_or_else(|| syntax(text))?;
    let (integer, fraction) = number.split_once('.').unwrap_or((&number, ""));
    let all_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if (integer.is_empty() && fraction.is_empty()) || !all_digits(integer) || !all_digits(fraction)
    {
        return Err(syntax(text));
    }
    let exponent = match exponent {
        None => 0i64,
        Some(raw) => {
            let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
            let unsigned = strip_separators(unsigned, 10).ok_or_else(|| syntax(text))?;
            if unsigned.is_empty() || !all_digits(&unsigned) {
                return Err(syntax(text));
            }
            let magnitude = unsigned.trim_start_matches('0').parse::<i64>().unwrap_or(
                if unsigned.trim_start_matches('0').is_empty() {
                    0
                } else {
                    i64::MAX
                },
            );
            if magnitude > i64::from(i32::MAX / 2) {
                return Err(overflow());
            }
            if raw.starts_with('-') {
                -magnitude
            } else {
                magnitude
            }
        }
    };
    let digits = format!("{integer}{fraction}");
    let magnitude = BigInt::parse_bytes(digits.as_bytes(), 10).unwrap_or_default();
    let mantissa = if negative { -magnitude } else { magnitude };
    let scale = i64::try_from(fraction.len()).unwrap_or(i64::MAX) - exponent;
    if scale > i64::from(MAX_DSCALE) {
        return Err(overflow());
    }
    if scale < 0 {
        let shift = u32::try_from(-scale).map_err(|_| overflow())?;
        if shift as usize + digits.len() > MAX_INTEGER_DIGITS + 1 && !mantissa.is_zero() {
            return Err(overflow());
        }
        return Numeric::finite(mantissa * pow10(shift), 0).check_format();
    }
    Numeric::finite(mantissa, u32::try_from(scale).unwrap_or_default()).check_format()
}

/// `numeric_recv`.
///
/// # Errors
/// Returns a message for malformed frames.
pub fn receive_binary(bytes: &[u8]) -> Result<Numeric, String> {
    let word = |index: usize| -> Result<u16, String> {
        bytes
            .get(index * 2..index * 2 + 2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .ok_or_else(|| "insufficient data left in message".to_string())
    };
    let count = usize::from(word(0)?);
    let weight = i64::from(i16::from_be_bytes(word(1)?.to_be_bytes()));
    let sign = word(2)?;
    let scale = word(3)?;
    if bytes.len() != 8 + count * 2 {
        return Err("invalid length in external \"numeric\" value".to_string());
    }
    match sign {
        SIGN_NAN => return Ok(Numeric::NaN),
        SIGN_POSITIVE_INFINITY => return Ok(Numeric::Infinity),
        SIGN_NEGATIVE_INFINITY => return Ok(Numeric::NegInfinity),
        SIGN_POSITIVE | SIGN_NEGATIVE => {}
        _ => return Err("invalid sign in external \"numeric\" value".to_string()),
    }
    if u32::from(scale) > MAX_DSCALE {
        return Err("invalid scale in external \"numeric\" value".to_string());
    }
    let mut value = BigInt::zero();
    for index in 0..count {
        let digit = word(4 + index)?;
        if digit >= 10_000 {
            return Err("invalid digit in external \"numeric\" value".to_string());
        }
        value = value * 10_000u32 + u32::from(digit);
    }
    // `value` carries 4 * (count - 1 - weight) fractional decimal digits.
    let represented = 4 * (i64::try_from(count).unwrap_or_default() - 1 - weight);
    let scale_i = i64::from(scale);
    let mantissa = if represented <= scale_i {
        value * pow10(u32::try_from(scale_i - represented).map_err(|_| "invalid weight")?)
    } else {
        value / pow10(u32::try_from(represented - scale_i).map_err(|_| "invalid weight")?)
    };
    let mantissa = if sign == SIGN_NEGATIVE {
        -mantissa
    } else {
        mantissa
    };
    Numeric::finite(mantissa, u32::from(scale))
        .check_format()
        .map_err(|error| error.message)
}

/// `numeric_in` followed by the declared typmod.
///
/// # Errors
/// Returns PostgreSQL's input or field-overflow error.
pub fn parse_with_typmod(
    text: &str,
    typmod: Option<NumericTypmod>,
) -> Result<Numeric, TypeInputError> {
    parse_text(text)?.apply_typmod(typmod)
}

/// `1` as a numeric.
#[must_use]
pub fn one() -> Numeric {
    Numeric::finite(BigInt::one(), 0)
}

#[cfg(test)]
#[path = "numeric_tests.rs"]
mod tests;

/// Resolves `numeric`, `decimal`, `dec` and their `(p)` / `(p, s)` modifiers.
/// Returns `None` for other type names.
///
/// # Errors
/// Returns PostgreSQL's typmod messages for out-of-range modifiers.
pub fn parse_type_name(lower: &str) -> Option<Result<Option<NumericTypmod>, String>> {
    let (base, modifier) = lower
        .split_once('(')
        .map_or((lower.trim(), None), |(base, rest)| {
            (base.trim(), Some(rest))
        });
    if !matches!(base, "numeric" | "decimal" | "dec") {
        return None;
    }
    let Some(modifier) = modifier else {
        return Some(Ok(None));
    };
    let Some(inner) = modifier.trim_end().strip_suffix(')') else {
        return Some(Err(format!("unsupported modifier in '{lower}'")));
    };
    let parts = inner
        .split(',')
        .map(|part| part.trim().parse::<i64>())
        .collect::<Result<Vec<_>, _>>();
    let typmod = match parts.as_deref() {
        Ok([precision]) => NumericTypmod::new(*precision, 0),
        Ok([precision, scale]) => NumericTypmod::new(*precision, *scale),
        Ok(_) => return Some(Err("invalid NUMERIC type modifier".to_string())),
        Err(_) => return Some(Err(format!("invalid NUMERIC type modifier in '{lower}'"))),
    };
    Some(typmod.map(Some).map_err(|error| error.message))
}
