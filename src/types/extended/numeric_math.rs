//! `numeric` arithmetic with PostgreSQL's result scales (`numeric.c`).
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::{pow10, Numeric, TypeInputError, MAX_DISPLAY_SCALE, MAX_DSCALE};

/// `NUMERIC_MIN_SIG_DIGITS`.
const MIN_SIG_DIGITS: i64 = 16;

fn error(sqlstate: &'static str, message: &str) -> TypeInputError {
    TypeInputError {
        sqlstate,
        message: message.to_string(),
    }
}

fn division_by_zero() -> TypeInputError {
    error("22012", "division by zero")
}

fn clamp_scale(scale: i64) -> u32 {
    u32::try_from(scale.clamp(0, i64::from(MAX_DISPLAY_SCALE))).unwrap_or_default()
}

/// Both operands with a shared display scale.
fn aligned(
    left: &BigInt,
    left_scale: u32,
    right: &BigInt,
    right_scale: u32,
) -> (BigInt, BigInt, u32) {
    let scale = left_scale.max(right_scale);
    (
        Numeric::rescaled_mantissa(left, left_scale, scale),
        Numeric::rescaled_mantissa(right, right_scale, scale),
        scale,
    )
}

/// `numeric_add`.
///
/// # Errors
/// Returns `22003` when the sum overflows the numeric format.
pub fn add(left: &Numeric, right: &Numeric) -> Result<Numeric, TypeInputError> {
    use Numeric::{Finite, Infinity, NaN, NegInfinity};
    match (left, right) {
        (NaN, _) | (_, NaN) | (Infinity, NegInfinity) | (NegInfinity, Infinity) => Ok(NaN),
        (Infinity, _) | (_, Infinity) => Ok(Infinity),
        (NegInfinity, _) | (_, NegInfinity) => Ok(NegInfinity),
        (
            Finite {
                mantissa: a,
                scale: a_scale,
            },
            Finite {
                mantissa: b,
                scale: b_scale,
            },
        ) => {
            let (a, b, scale) = aligned(a, *a_scale, b, *b_scale);
            Numeric::finite(a + b, scale).check_format()
        }
    }
}

/// `numeric_uminus`.
#[must_use]
pub fn negate(value: &Numeric) -> Numeric {
    match value {
        Numeric::NaN => Numeric::NaN,
        Numeric::Infinity => Numeric::NegInfinity,
        Numeric::NegInfinity => Numeric::Infinity,
        Numeric::Finite { mantissa, scale } => Numeric::finite(-mantissa, *scale),
    }
}

/// `numeric_abs`.
#[must_use]
pub fn abs(value: &Numeric) -> Numeric {
    match value {
        Numeric::NegInfinity => Numeric::Infinity,
        Numeric::Finite { mantissa, scale } => Numeric::finite(mantissa.abs(), *scale),
        other => other.clone(),
    }
}

/// `numeric_sub`.
///
/// # Errors
/// Returns `22003` when the difference overflows the numeric format.
pub fn subtract(left: &Numeric, right: &Numeric) -> Result<Numeric, TypeInputError> {
    add(left, &negate(right))
}

/// `numeric_mul`: the result scale is the sum of the operand scales.
///
/// # Errors
/// Returns `22003` when the product overflows the numeric format.
pub fn multiply(left: &Numeric, right: &Numeric) -> Result<Numeric, TypeInputError> {
    match (left, right) {
        (
            Numeric::Finite {
                mantissa: a,
                scale: a_scale,
            },
            Numeric::Finite {
                mantissa: b,
                scale: b_scale,
            },
        ) => {
            let scale = a_scale + b_scale;
            let product = Numeric::finite(a * b, scale);
            if scale > MAX_DSCALE {
                return product
                    .round_to(i32::try_from(MAX_DSCALE).unwrap_or_default())
                    .check_format();
            }
            product.check_format()
        }
        _ => Ok(special_product(left, right)),
    }
}

fn special_product(left: &Numeric, right: &Numeric) -> Numeric {
    match (left.signum(), right.signum()) {
        (Some(left), Some(right)) if left != 0 && right != 0 => {
            if left == right {
                Numeric::Infinity
            } else {
                Numeric::NegInfinity
            }
        }
        _ => Numeric::NaN,
    }
}

/// The base-10000 weight and first digit of a finite nonzero value.
fn weight_and_first(mantissa: &BigInt, scale: u32) -> (i64, i64) {
    let (digits, weight) = Numeric::base_digits(mantissa, scale);
    (
        i64::from(weight),
        digits.first().copied().map_or(0, i64::from),
    )
}

/// `select_div_scale`.
fn select_div_scale(a: &BigInt, a_scale: u32, b: &BigInt, b_scale: u32) -> u32 {
    let (weight1, first1) = if a.is_zero() {
        (0, 0)
    } else {
        weight_and_first(a, a_scale)
    };
    let (weight2, first2) = if b.is_zero() {
        (0, 0)
    } else {
        weight_and_first(b, b_scale)
    };
    let mut quotient_weight = weight1 - weight2;
    if first1 <= first2 {
        quotient_weight -= 1;
    }
    let scale = (MIN_SIG_DIGITS - quotient_weight * 4)
        .max(i64::from(a_scale))
        .max(i64::from(b_scale));
    clamp_scale(scale)
}

/// Rounds `numerator / denominator` half away from zero.
fn rounded_quotient(numerator: &BigInt, denominator: &BigInt) -> BigInt {
    let (quotient, remainder) = numerator.div_rem(denominator);
    if remainder.magnitude() * 2u8 >= *denominator.magnitude() {
        if numerator.is_negative() == denominator.is_negative() {
            quotient + 1
        } else {
            quotient - 1
        }
    } else {
        quotient
    }
}

/// Divides finite values to `scale` digits, rounding half away from zero.
fn divide_finite(a: &BigInt, a_scale: u32, b: &BigInt, b_scale: u32, scale: u32) -> Numeric {
    // a/10^as / (b/10^bs) * 10^scale = a * 10^(scale + bs - as) / b
    let shift = i64::from(scale) + i64::from(b_scale) - i64::from(a_scale);
    let (numerator, denominator) = if shift >= 0 {
        (
            a * pow10(u32::try_from(shift).unwrap_or_default()),
            b.clone(),
        )
    } else {
        (
            a.clone(),
            b * pow10(u32::try_from(-shift).unwrap_or_default()),
        )
    };
    Numeric::finite(rounded_quotient(&numerator, &denominator), scale)
}

/// `numeric_div` with `select_div_scale`.
///
/// # Errors
/// Returns `22012` for a zero divisor and `22003` on overflow.
pub fn divide(left: &Numeric, right: &Numeric) -> Result<Numeric, TypeInputError> {
    match (left, right) {
        (Numeric::NaN, _) | (_, Numeric::NaN) => Ok(Numeric::NaN),
        (_, divisor) if divisor.is_zero() => Err(division_by_zero()),
        (
            Numeric::Finite {
                mantissa: a,
                scale: a_scale,
            },
            Numeric::Finite {
                mantissa: b,
                scale: b_scale,
            },
        ) => {
            let scale = select_div_scale(a, *a_scale, b, *b_scale);
            divide_finite(a, *a_scale, b, *b_scale, scale).check_format()
        }
        (Numeric::Finite { .. }, _) => Ok(Numeric::zero()),
        (_, Numeric::Finite { .. }) => Ok(special_product(left, right)),
        _ => Ok(Numeric::NaN),
    }
}

/// `numeric_div_trunc` (`div()`): the integral quotient truncated toward zero.
///
/// # Errors
/// Returns `22012` for a zero divisor.
pub fn divide_truncated(left: &Numeric, right: &Numeric) -> Result<Numeric, TypeInputError> {
    match (left, right) {
        (Numeric::NaN, _) | (_, Numeric::NaN) => Ok(Numeric::NaN),
        (_, divisor) if divisor.is_zero() => Err(division_by_zero()),
        (
            Numeric::Finite {
                mantissa: a,
                scale: a_scale,
            },
            Numeric::Finite {
                mantissa: b,
                scale: b_scale,
            },
        ) => {
            let (a, b, _) = aligned(a, *a_scale, b, *b_scale);
            Ok(Numeric::finite(a / b, 0))
        }
        (Numeric::Finite { .. }, _) => Ok(Numeric::zero()),
        (_, Numeric::Finite { .. }) => Ok(special_product(left, right)),
        _ => Ok(Numeric::NaN),
    }
}

/// `numeric_mod`: the remainder carries the dividend's sign.
///
/// # Errors
/// Returns `22012` for a zero divisor.
pub fn modulo(left: &Numeric, right: &Numeric) -> Result<Numeric, TypeInputError> {
    match (left, right) {
        (Numeric::NaN, _) | (_, Numeric::NaN) => Ok(Numeric::NaN),
        (_, divisor) if divisor.is_zero() => Err(division_by_zero()),
        (
            Numeric::Finite {
                mantissa: a,
                scale: a_scale,
            },
            Numeric::Finite {
                mantissa: b,
                scale: b_scale,
            },
        ) => {
            let (a, b, scale) = aligned(a, *a_scale, b, *b_scale);
            Ok(Numeric::finite(a % b, scale))
        }
        (Numeric::Finite { .. }, _) => Ok(left.clone()),
        _ => Ok(Numeric::NaN),
    }
}

/// `numeric_round` / `numeric_trunc` scale arguments clamp to the format.
fn clamp_round_scale(scale: i64) -> i32 {
    i32::try_from(scale.clamp(
        -i64::from(MAX_DISPLAY_SCALE) * 132,
        i64::from(MAX_DISPLAY_SCALE),
    ))
    .unwrap_or_default()
}

/// `round(numeric, int)`.
///
/// # Errors
/// Returns `22003` when rounding carries past the numeric format.
pub fn round(value: &Numeric, scale: i64) -> Result<Numeric, TypeInputError> {
    value.round_to(clamp_round_scale(scale)).check_format()
}

/// `trunc(numeric, int)`.
#[must_use]
pub fn trunc(value: &Numeric, scale: i64) -> Numeric {
    value.truncate_to(clamp_round_scale(scale))
}

/// `ceil(numeric)`.
#[must_use]
pub fn ceil(value: &Numeric) -> Numeric {
    let truncated = value.truncate_to(0);
    if value.compare(&truncated).is_gt() {
        add(&truncated, &super::one()).unwrap_or(truncated)
    } else {
        truncated
    }
}

/// `floor(numeric)`.
#[must_use]
pub fn floor(value: &Numeric) -> Numeric {
    let truncated = value.truncate_to(0);
    if value.compare(&truncated).is_lt() {
        subtract(&truncated, &super::one()).unwrap_or(truncated)
    } else {
        truncated
    }
}

/// `sign(numeric)`.
#[must_use]
pub fn sign(value: &Numeric) -> Numeric {
    value
        .signum()
        .map_or(Numeric::NaN, |sign| Numeric::from_i64(i64::from(sign)))
}

/// `min_scale(numeric)`: the smallest scale that represents the value exactly.
#[must_use]
pub fn min_scale(value: &Numeric) -> Option<u32> {
    let Numeric::Finite { mantissa, scale } = value else {
        return None;
    };
    let mut mantissa = mantissa.clone();
    let mut scale = *scale;
    let ten = BigInt::from(10u8);
    while scale > 0 && !mantissa.is_zero() {
        let (quotient, remainder) = mantissa.div_rem(&ten);
        if !remainder.is_zero() {
            break;
        }
        mantissa = quotient;
        scale -= 1;
    }
    if mantissa.is_zero() {
        scale = 0;
    }
    Some(scale)
}

/// `trim_scale(numeric)`.
#[must_use]
pub fn trim_scale(value: &Numeric) -> Numeric {
    min_scale(value).map_or_else(
        || value.clone(),
        |scale| value.round_to(i32::try_from(scale).unwrap_or_default()),
    )
}

/// The base-10000 `ndigits`/`weight` as an approximate double (`numericvar_to_double`).
fn approximate(value: &Numeric) -> f64 {
    value.to_f64()
}

/// `sqrt(numeric)`.
///
/// # Errors
/// Returns `2201F` for negative input.
pub fn sqrt(value: &Numeric) -> Result<Numeric, TypeInputError> {
    let Numeric::Finite { mantissa, scale } = value else {
        return match value {
            Numeric::NegInfinity => Err(negative_sqrt()),
            other => Ok(other.clone()),
        };
    };
    if mantissa.is_negative() {
        return Err(negative_sqrt());
    }
    let weight = if mantissa.is_zero() {
        0
    } else {
        weight_and_first(mantissa, *scale).0
    };
    let sweight = (weight + 1) * 4 / 2 - 1;
    let result_scale = clamp_scale((MIN_SIG_DIGITS - sweight).max(i64::from(*scale)));
    // round(sqrt(m / 10^s) * 10^r) = round(sqrt(m * 10^(2r - s)))
    let radicand = mantissa * pow10(2 * result_scale - *scale);
    let doubled = (radicand * 4u8).sqrt();
    Ok(Numeric::finite((doubled + 1u8) / 2u8, result_scale))
}

fn negative_sqrt() -> TypeInputError {
    error("2201F", "cannot take square root of a negative number")
}

/// A fixed-point working value: `value / 10^digits`.
struct Fixed {
    value: BigInt,
    digits: u32,
}

impl Fixed {
    fn from_numeric(mantissa: &BigInt, scale: u32, digits: u32) -> Self {
        Self {
            value: Numeric::rescaled_mantissa(mantissa, scale, digits),
            digits,
        }
    }

    fn one(&self) -> BigInt {
        pow10(self.digits)
    }

    fn multiply(&self, left: &BigInt, right: &BigInt) -> BigInt {
        (left * right) / self.one()
    }

    fn divide(&self, left: &BigInt, right: &BigInt) -> BigInt {
        (left * self.one()) / right
    }

    fn into_numeric(self, scale: u32) -> Numeric {
        Numeric::finite(self.value, self.digits).round_to(i32::try_from(scale).unwrap_or_default())
    }
}

/// `exp(x)` for a fixed-point `x`, by halving then squaring.
fn exp_fixed(work: &Fixed, x: &BigInt) -> BigInt {
    let one = work.one();
    let mut reduced = x.clone();
    let mut halvings = 0u32;
    let half = &one / 2u8;
    while reduced.magnitude() > half.magnitude() {
        reduced /= 2u8;
        halvings += 1;
    }
    let mut term = one.clone();
    let mut sum = one.clone();
    let mut n = 1u32;
    loop {
        term = work.multiply(&term, &reduced) / n;
        if term.is_zero() {
            break;
        }
        sum += &term;
        n += 1;
    }
    for _ in 0..halvings {
        sum = work.multiply(&sum, &sum);
    }
    sum
}

/// `ln(x)` for a positive fixed-point `x`.
fn ln_fixed(work: &Fixed, x: &BigInt) -> BigInt {
    let one = work.one();
    // Reduce x into [0.5, 2] with powers of ten, then take square roots.
    let mut x = x.clone();
    let mut tens = 0i64;
    let ten_x = &one * 10u8;
    while x > ten_x {
        x /= 10u8;
        tens += 1;
    }
    while x < one.clone() / 10u8 {
        x *= 10u8;
        tens -= 1;
    }
    let mut roots = 0u32;
    let low = &one * 9u8 / 10u8;
    let high = &one * 11u8 / 10u8;
    while x < low || x > high {
        x = (&x * &one).sqrt();
        roots += 1;
    }
    // ln(x) = 2 atanh((x-1)/(x+1))
    let z = work.divide(&(&x - &one), &(&x + &one));
    let z2 = work.multiply(&z, &z);
    let mut term = z.clone();
    let mut sum = z;
    let mut k = 3u32;
    loop {
        term = work.multiply(&term, &z2);
        let step = &term / k;
        if step.is_zero() {
            break;
        }
        sum += step;
        k += 2;
    }
    let mut result = sum * 2u8 * (BigInt::one() << roots);
    if tens != 0 {
        let ten = Fixed::from_numeric(&BigInt::from(10u8), 0, work.digits);
        result += ln_fixed(work, &ten.value) * tens;
    }
    result
}

/// `estimate_ln_dweight`.
fn estimate_ln_dweight(value: &Numeric) -> i64 {
    let Numeric::Finite { mantissa, scale } = value else {
        return 0;
    };
    if !mantissa.is_positive() {
        return 0;
    }
    let low = Numeric::finite(BigInt::from(9u8), 1);
    let high = Numeric::finite(BigInt::from(11u8), 1);
    if value.compare(&low).is_ge() && value.compare(&high).is_le() {
        let Ok(Numeric::Finite {
            mantissa: difference,
            scale: difference_scale,
        }) = subtract(value, &super::one())
        else {
            return 0;
        };
        if difference.is_zero() {
            return 0;
        }
        let (digits, weight) = Numeric::base_digits(&difference, difference_scale);
        let first = f64::from(digits.first().copied().unwrap_or(1));
        return i64::from(weight) * 4 + first.log10().trunc().to_i64().unwrap_or_default();
    }
    let (digits, weight) = Numeric::base_digits(mantissa, *scale);
    let mut leading = f64::from(digits.first().copied().unwrap_or(0));
    let mut decimal_weight = f64::from(weight) * 4.0;
    if let Some(second) = digits.get(1) {
        leading = leading * 10_000.0 + f64::from(*second);
        decimal_weight -= 4.0;
    }
    let ln = leading.ln() + decimal_weight * std::f64::consts::LN_10;
    ln.abs().log10().trunc().to_i64().unwrap_or_default()
}

fn ln_checked(value: &Numeric) -> Result<(BigInt, u32), TypeInputError> {
    match value {
        Numeric::Finite { mantissa, scale } if mantissa.is_positive() => {
            Ok((mantissa.clone(), *scale))
        }
        Numeric::Finite { mantissa, .. } if mantissa.is_zero() => {
            Err(error("2201E", "cannot take logarithm of zero"))
        }
        _ => Err(error("2201E", "cannot take logarithm of a negative number")),
    }
}

/// Working digits for a transcendental result at `scale`.
fn working_digits(scale: u32, magnitude: i64) -> u32 {
    u32::try_from(i64::from(scale) + magnitude.max(0) + 20).unwrap_or(u32::MAX)
}

/// `ln(numeric)`.
///
/// # Errors
/// Returns `2201E` for zero or negative input.
pub fn ln(value: &Numeric) -> Result<Numeric, TypeInputError> {
    match value {
        Numeric::NaN | Numeric::Infinity => return Ok(value.clone()),
        Numeric::NegInfinity => {
            return Err(error("2201E", "cannot take logarithm of a negative number"))
        }
        Numeric::Finite { .. } => {}
    }
    let (mantissa, scale) = ln_checked(value)?;
    let result_scale =
        clamp_scale((MIN_SIG_DIGITS - estimate_ln_dweight(value)).max(i64::from(scale)));
    let work = Fixed {
        value: BigInt::zero(),
        digits: working_digits(result_scale, 0),
    };
    let x = Fixed::from_numeric(&mantissa, scale, work.digits);
    let value = ln_fixed(&work, &x.value);
    Ok(Fixed {
        value,
        digits: work.digits,
    }
    .into_numeric(result_scale))
}

/// `log(base, numeric)`.
///
/// # Errors
/// Returns `2201E` for invalid logarithm inputs and `22012` for base 1.
pub fn log(base: &Numeric, value: &Numeric) -> Result<Numeric, TypeInputError> {
    if base.is_nan() || value.is_nan() {
        return Ok(Numeric::NaN);
    }
    let (base_mantissa, base_scale) = ln_checked(base)?;
    let (mantissa, scale) = ln_checked(value)?;
    let result_dweight = estimate_ln_dweight(value) - estimate_ln_dweight(base);
    let result_scale = clamp_scale(
        (MIN_SIG_DIGITS - result_dweight)
            .max(i64::from(base_scale))
            .max(i64::from(scale)),
    );
    let work = Fixed {
        value: BigInt::zero(),
        digits: working_digits(result_scale, result_dweight.abs()),
    };
    let ln_base = ln_fixed(
        &work,
        &Fixed::from_numeric(&base_mantissa, base_scale, work.digits).value,
    );
    if ln_base.is_zero() {
        return Err(division_by_zero());
    }
    let ln_value = ln_fixed(
        &work,
        &Fixed::from_numeric(&mantissa, scale, work.digits).value,
    );
    let quotient = work.divide(&ln_value, &ln_base);
    Ok(Fixed {
        value: quotient,
        digits: work.digits,
    }
    .into_numeric(result_scale))
}

/// `exp(numeric)`.
///
/// # Errors
/// Returns `22003` when the result overflows the numeric format.
pub fn exp(value: &Numeric) -> Result<Numeric, TypeInputError> {
    let Numeric::Finite { mantissa, scale } = value else {
        return Ok(match value {
            Numeric::NegInfinity => Numeric::zero(),
            other => other.clone(),
        });
    };
    let approximate_weight =
        (approximate(value) * std::f64::consts::LOG10_E).clamp(-2000.0, 2000.0);
    if approximate_weight > 131_072.0 * 0.999 {
        return Err(error("22003", "value overflows numeric format"));
    }
    let weight = approximate_weight.trunc().to_i64().unwrap_or_default();
    let result_scale = clamp_scale((MIN_SIG_DIGITS - weight).max(i64::from(*scale)));
    let work = Fixed {
        value: BigInt::zero(),
        digits: working_digits(result_scale, weight),
    };
    let x = Fixed::from_numeric(mantissa, *scale, work.digits);
    let value = exp_fixed(&work, &x.value);
    Fixed {
        value,
        digits: work.digits,
    }
    .into_numeric(result_scale)
    .check_format()
}

/// `power(numeric, numeric)` / `^`.
///
/// # Errors
/// Returns PostgreSQL's errors for undefined or complex powers and overflow.
pub fn power(base: &Numeric, exponent: &Numeric) -> Result<Numeric, TypeInputError> {
    if base.is_nan() || exponent.is_nan() {
        if base.compare(&super::one()).is_eq() || exponent.is_zero() {
            return Ok(super::one());
        }
        return Ok(Numeric::NaN);
    }
    let (
        Numeric::Finite {
            mantissa: base_mantissa,
            scale: base_scale,
        },
        Numeric::Finite {
            mantissa: exponent_mantissa,
            scale: exponent_scale,
        },
    ) = (base, exponent)
    else {
        return Ok(
            Numeric::from_f64(approximate(base).powf(approximate(exponent)))
                .unwrap_or(Numeric::NaN),
        );
    };
    if base_mantissa.is_zero() && exponent_mantissa.is_negative() {
        return Err(error(
            "2201F",
            "zero raised to a negative power is undefined",
        ));
    }
    let integral = exponent.truncate_to(0).compare(exponent).is_eq();
    if base_mantissa.is_negative() && !integral {
        return Err(error(
            "2201F",
            "a negative number raised to a non-integer power yields a complex result",
        ));
    }
    if integral {
        if let Some(power) = exponent
            .to_i64_rounded()
            .ok()
            .flatten()
            .and_then(|value| i32::try_from(value).ok())
        {
            return power_integer(base_mantissa, *base_scale, power, *exponent_scale);
        }
    }
    if base_mantissa.is_zero() {
        return Ok(Numeric::finite(BigInt::zero(), 16));
    }
    let ln_estimate = approximate(base).ln() * approximate(exponent);
    if ln_estimate.abs() > 131_072.0 * 3.01 * std::f64::consts::LN_10 {
        if ln_estimate > 0.0 {
            return Err(error("22003", "value overflows numeric format"));
        }
        return Ok(Numeric::finite(BigInt::zero(), MAX_DISPLAY_SCALE));
    }
    let weight = (ln_estimate * std::f64::consts::LOG10_E)
        .trunc()
        .to_i64()
        .unwrap_or_default();
    let result_scale = clamp_scale(
        (MIN_SIG_DIGITS - weight)
            .max(i64::from(*base_scale))
            .max(i64::from(*exponent_scale)),
    );
    let work = Fixed {
        value: BigInt::zero(),
        digits: working_digits(result_scale, weight.abs() * 2),
    };
    let ln_base = ln_fixed(
        &work,
        &Fixed::from_numeric(base_mantissa, *base_scale, work.digits).value,
    );
    let exponent_fixed = Fixed::from_numeric(exponent_mantissa, *exponent_scale, work.digits);
    let product = work.multiply(&ln_base, &exponent_fixed.value);
    Fixed {
        value: exp_fixed(&work, &product),
        digits: work.digits,
    }
    .into_numeric(result_scale)
    .check_format()
}

/// `power_var_int`.
fn power_integer(
    mantissa: &BigInt,
    scale: u32,
    exponent: i32,
    exponent_scale: u32,
) -> Result<Numeric, TypeInputError> {
    let base = Numeric::finite(mantissa.clone(), scale);
    let estimate = if mantissa.is_zero() {
        0.0
    } else {
        let (digits, weight) = Numeric::base_digits(mantissa, scale);
        let mut leading = 0.0f64;
        let mut decimal_weight = f64::from(weight) * 4.0;
        for (index, digit) in digits.iter().enumerate() {
            if index > 0 && index * 4 >= 16 {
                break;
            }
            if index > 0 {
                decimal_weight -= 4.0;
            }
            leading = leading * 10_000.0 + f64::from(*digit);
        }
        f64::from(exponent) * (leading.log10() + decimal_weight)
    };
    if estimate > 131_072.0 {
        return Err(error("22003", "value overflows numeric format"));
    }
    if estimate + 1.0 < -f64::from(MAX_DISPLAY_SCALE) {
        return Ok(Numeric::finite(BigInt::zero(), MAX_DISPLAY_SCALE));
    }
    let result_scale = clamp_scale(
        (MIN_SIG_DIGITS - estimate.trunc().to_i64().unwrap_or_default())
            .max(i64::from(scale))
            .max(i64::from(exponent_scale)),
    );
    let magnitude = exponent.unsigned_abs();
    if exponent == 0 {
        return Ok(Numeric::finite(pow10(result_scale), result_scale));
    }
    let raised = num_traits::pow(mantissa.clone(), magnitude as usize);
    let raised_scale = scale.saturating_mul(magnitude);
    if exponent > 0 {
        return Numeric::finite(raised, raised_scale)
            .round_to(i32::try_from(result_scale).unwrap_or_default())
            .check_format();
    }
    if raised.is_zero() {
        return Err(division_by_zero());
    }
    let _ = base;
    divide_finite(&BigInt::one(), 0, &raised, raised_scale, result_scale).check_format()
}

/// `factorial(bigint)`.
///
/// # Errors
/// Returns `2201F` for negative input and `22003` on overflow.
pub fn factorial(value: i64) -> Result<Numeric, TypeInputError> {
    if value < 0 {
        return Err(error(
            "2201F",
            "factorial of a negative number is undefined",
        ));
    }
    if value > 32_177 {
        return Err(error("22003", "value overflows numeric format"));
    }
    let mut product = BigInt::one();
    for factor in 2..=value {
        product *= factor;
    }
    Ok(Numeric::finite(product, 0))
}

/// `width_bucket(numeric, numeric, numeric, int)`.
///
/// # Errors
/// Returns `2201G` for invalid bounds or bucket counts.
pub fn width_bucket(
    operand: &Numeric,
    low: &Numeric,
    high: &Numeric,
    count: i64,
) -> Result<i64, TypeInputError> {
    if count <= 0 {
        return Err(error("2201G", "count must be greater than zero"));
    }
    if operand.is_nan() || low.is_nan() || high.is_nan() {
        return Err(error(
            "2201G",
            "operand, lower bound, and upper bound cannot be NaN",
        ));
    }
    if !matches!(low, Numeric::Finite { .. }) || !matches!(high, Numeric::Finite { .. }) {
        return Err(error("2201G", "lower and upper bounds must be finite"));
    }
    let ordering = low.compare(high);
    if ordering.is_eq() {
        return Err(error("2201G", "lower bound cannot equal upper bound"));
    }
    let count_numeric = Numeric::from_i64(count);
    let ascending = ordering.is_lt();
    let (below, above) = if ascending {
        (operand.compare(low).is_lt(), operand.compare(high).is_ge())
    } else {
        (operand.compare(low).is_gt(), operand.compare(high).is_le())
    };
    if below {
        return Ok(0);
    }
    if above {
        return count
            .checked_add(1)
            .ok_or_else(|| error("22003", "integer out of range"));
    }
    let (numerator, denominator) = if ascending {
        (subtract(operand, low)?, subtract(high, low)?)
    } else {
        (subtract(low, operand)?, subtract(low, high)?)
    };
    let scaled = multiply(&numerator, &count_numeric)?;
    let (
        Numeric::Finite {
            mantissa: a,
            scale: a_scale,
        },
        Numeric::Finite {
            mantissa: b,
            scale: b_scale,
        },
    ) = (&scaled, &denominator)
    else {
        return Ok(0);
    };
    let (a, b, _) = aligned(a, *a_scale, b, *b_scale);
    (a / b + 1u8)
        .to_i64()
        .ok_or_else(|| error("22003", "integer out of range"))
}

#[cfg(test)]
#[path = "numeric_math_tests.rs"]
mod tests;
