//! Exact numeric powers, with bounded integer coefficients and logarithm estimates.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::super::{one, pow10, Numeric, TypeInputError, MAX_DISPLAY_SCALE, MAX_INTEGER_DIGITS};
use super::{
    abs, clamp_scale, divide_finite, division_by_zero, error, working_digits, Fixed,
    MAX_EXP_ARGUMENT, MIN_SIG_DIGITS,
};

/// `power(numeric, numeric)` / `^`.
///
/// # Errors
/// Returns PostgreSQL's errors for undefined or complex powers and overflow.
pub fn power(base: &Numeric, exponent: &Numeric) -> Result<Numeric, TypeInputError> {
    match power_with_check(base, exponent, &mut || {
        Ok::<(), std::convert::Infallible>(())
    }) {
        Ok(value) => Ok(value),
        Err(PowerError::TypeInput(error)) => Err(error),
        Err(PowerError::Interruption(never)) => match never {},
    }
}

pub(crate) enum PowerError<E> {
    TypeInput(TypeInputError),
    Interruption(E),
}

impl<E> From<TypeInputError> for PowerError<E> {
    fn from(error: TypeInputError) -> Self {
        Self::TypeInput(error)
    }
}

pub(crate) fn power_with_check<E>(
    base: &Numeric,
    exponent: &Numeric,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Numeric, PowerError<E>> {
    check().map_err(PowerError::Interruption)?;
    if base.is_nan() || exponent.is_nan() {
        if base.compare(&one()).is_eq() || exponent.is_zero() {
            return Ok(one());
        }
        return Ok(Numeric::NaN);
    }
    let integer_exponent = exponent.truncate_to(0);
    let integral = integer_exponent.compare(exponent).is_eq();
    if base.is_zero() && exponent.signum() == Some(-1) {
        return Err(error("2201F", "zero raised to a negative power is undefined").into());
    }
    if base.signum() == Some(-1) && !integral {
        return Err(error(
            "2201F",
            "a negative number raised to a non-integer power yields a complex result",
        )
        .into());
    }
    let odd = matches!(
        &integer_exponent,
        Numeric::Finite { mantissa, .. } if mantissa.is_odd()
    );
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
        return Ok(power_infinite(base, exponent, odd));
    };
    let integer_power = if integral {
        exponent
            .to_i64_rounded()
            .ok()
            .flatten()
            .and_then(|value| i32::try_from(value).ok())
    } else {
        None
    };
    if let Some(power) = integer_power.filter(|power| integer_workspace_fits(base_mantissa, *power))
    {
        return power_integer(base_mantissa, *base_scale, power, *exponent_scale, check);
    }
    if base_mantissa.is_zero() {
        return Ok(Numeric::finite(BigInt::zero(), 16));
    }
    power_transcendental(
        (base_mantissa, *base_scale),
        (exponent_mantissa, *exponent_scale),
        integer_power.is_some(),
        base_mantissa.is_negative() && odd,
        check,
    )
}

fn power_transcendental<E>(
    (base_mantissa, base_scale): (&BigInt, u32),
    (exponent_mantissa, exponent_scale): (&BigInt, u32),
    integer_branch: bool,
    negative: bool,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Numeric, PowerError<E>> {
    check().map_err(PowerError::Interruption)?;
    // An integral exponent outside the i32 fast path still permits a negative
    // base. Evaluate its positive magnitude and restore the exact integer sign.
    let magnitude = base_mantissa.abs();
    let ln_estimate = logarithm_estimate(&magnitude, base_scale)
        * Numeric::finite(exponent_mantissa.clone(), exponent_scale).to_f64();
    check().map_err(PowerError::Interruption)?;
    if !integer_branch && ln_estimate > f64::from(MAX_EXP_ARGUMENT) + 20.0 {
        return Err(error("22003", "value overflows numeric format").into());
    }
    // A one-decimal margin keeps the approximation from rejecting a valid
    // boundary value; the exact coefficient is checked after evaluation.
    if ln_estimate > 131_073.0 * std::f64::consts::LN_10 {
        return Err(error("22003", "value overflows numeric format").into());
    }
    if ln_estimate < -(f64::from(MAX_DISPLAY_SCALE) + 2.0) * std::f64::consts::LN_10 {
        return Ok(Numeric::finite(BigInt::zero(), MAX_DISPLAY_SCALE));
    }
    let weight = (ln_estimate * std::f64::consts::LOG10_E)
        .trunc()
        .to_i64()
        .unwrap_or_default();
    let result_scale = clamp_scale(
        (MIN_SIG_DIGITS - weight)
            .max(i64::from(base_scale))
            .max(i64::from(exponent_scale)),
    );
    // Logarithm error is multiplied by the exponent. Fixed guard digits alone
    // lose published digits for bases close to one and large finite exponents.
    let (_, exponent_weight) = Numeric::base_digits(exponent_mantissa, exponent_scale);
    let exponent_guard =
        u32::try_from((i64::from(exponent_weight) + 1).max(0) * 4).unwrap_or_default();
    let digits = power_working_digits(result_scale, weight, base_scale, exponent_guard);
    power_refined(
        (&magnitude, base_scale),
        (exponent_mantissa, exponent_scale),
        integer_branch,
        negative,
        digits,
        exponent_guard,
        check,
    )
}

fn power_refined<E>(
    (magnitude, base_scale): (&BigInt, u32),
    (exponent_mantissa, exponent_scale): (&BigInt, u32),
    integer_branch: bool,
    negative: bool,
    mut digits: u32,
    exponent_guard: u32,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Numeric, PowerError<E>> {
    loop {
        check().map_err(PowerError::Interruption)?;
        let work = Fixed {
            value: BigInt::zero(),
            digits,
        };
        let ln_base = super::ln_fixed_with_check(
            &work,
            &Fixed::from_numeric(magnitude, base_scale, work.digits).value,
            check,
        )
        .map_err(PowerError::Interruption)?;
        check().map_err(PowerError::Interruption)?;
        let exponent_fixed = Fixed::from_numeric(exponent_mantissa, exponent_scale, work.digits);
        check().map_err(PowerError::Interruption)?;
        let product = work.multiply(&ln_base, &exponent_fixed.value);
        check().map_err(PowerError::Interruption)?;
        let actual_product = Numeric::finite(product.clone(), work.digits).to_f64();
        check().map_err(PowerError::Interruption)?;
        // Upstream's general power branch calls exp_var, whose original
        // argument is bounded independently of numeric's integer-digit range.
        // INT32 exponents keep their own wider range even when the private
        // workspace limit routes their evaluation through this fallback.
        if !integer_branch && actual_product >= f64::from(MAX_EXP_ARGUMENT) {
            return Err(error("22003", "value overflows numeric format").into());
        }
        if actual_product < -(f64::from(MAX_DISPLAY_SCALE) + 2.0) * std::f64::consts::LN_10 {
            return Ok(Numeric::finite(BigInt::zero(), MAX_DISPLAY_SCALE));
        }
        let actual_weight = (actual_product * std::f64::consts::LOG10_E)
            .trunc()
            .to_i64()
            .unwrap_or_default();
        let result_scale = clamp_scale(
            (MIN_SIG_DIGITS - actual_weight)
                .max(i64::from(base_scale))
                .max(i64::from(exponent_scale)),
        );
        // The first pass includes exponent guard digits, so its log product
        // can select the final display scale and working precision even when
        // converting the original exponent/base to FLOAT8 lost their range.
        let required =
            power_working_digits(result_scale, actual_weight, base_scale, exponent_guard);
        if required > work.digits {
            digits = required;
            continue;
        }
        if product.is_zero() {
            let one = pow10(result_scale);
            check().map_err(PowerError::Interruption)?;
            return Ok(Numeric::finite(
                if negative { -one } else { one },
                result_scale,
            ));
        }
        let one = work.one();
        check().map_err(PowerError::Interruption)?;
        let ln_ten = super::ln_fixed_with_check(&work, &(one * 10u8), check)
            .map_err(PowerError::Interruption)?;
        if product > &ln_ten * MAX_INTEGER_DIGITS {
            return Err(error("22003", "value overflows numeric format").into());
        }
        let value = super::exp_fixed_with_check(&work, &product, check)
            .map_err(PowerError::Interruption)?;
        check().map_err(PowerError::Interruption)?;
        return Fixed {
            value: if negative { -value } else { value },
            digits: work.digits,
        }
        .into_numeric(result_scale)
        .check_format()
        .map_err(PowerError::TypeInput);
    }
}

fn power_working_digits(
    result_scale: u32,
    weight: i64,
    base_scale: u32,
    exponent_guard: u32,
) -> u32 {
    // A finite tiny base must stay nonzero, and multiplying ln(base) by an
    // exponent must retain the final result's significant digits.
    working_digits(result_scale, weight.abs() * 2 + i64::from(exponent_guard))
        .max(base_scale.saturating_add(exponent_guard).saturating_add(20))
}

/// Special numeric powers retain exact integer parity rather than casting the
/// exponent to FLOAT8, which loses odd integers above 2^53.
fn power_infinite(base: &Numeric, exponent: &Numeric, odd: bool) -> Numeric {
    if matches!(exponent, Numeric::Infinity | Numeric::NegInfinity) {
        let magnitude = abs(base).compare(&one());
        if magnitude.is_eq() {
            return one();
        }
        return if magnitude.is_gt() == matches!(exponent, Numeric::Infinity) {
            Numeric::Infinity
        } else {
            Numeric::from_i64(0)
        };
    }
    if exponent.is_zero() {
        one()
    } else if exponent.signum() == Some(-1) {
        Numeric::from_i64(0)
    } else if matches!(base, Numeric::NegInfinity) && odd {
        Numeric::NegInfinity
    } else {
        Numeric::Infinity
    }
}

/// `power_var_int`.
fn power_integer<E>(
    mantissa: &BigInt,
    scale: u32,
    exponent: i32,
    exponent_scale: u32,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Numeric, PowerError<E>> {
    check().map_err(PowerError::Interruption)?;
    if mantissa.is_zero() {
        if exponent < 0 {
            return Err(division_by_zero().into());
        }
        let result_scale = clamp_scale(
            MIN_SIG_DIGITS
                .max(i64::from(scale))
                .max(i64::from(exponent_scale)),
        );
        return Ok(Numeric::finite(
            if exponent == 0 {
                pow10(result_scale)
            } else {
                BigInt::zero()
            },
            result_scale,
        ));
    }
    let estimate = integer_power_estimate(mantissa, scale, exponent);
    check().map_err(PowerError::Interruption)?;
    if estimate > 131_072.0 {
        return Err(error("22003", "value overflows numeric format").into());
    }
    if estimate + 1.0 < -f64::from(MAX_DISPLAY_SCALE) {
        return Ok(Numeric::finite(BigInt::zero(), MAX_DISPLAY_SCALE));
    }
    let result_scale = integer_power_result_scale(estimate, scale, exponent_scale);
    let magnitude = exponent.unsigned_abs();
    if exponent == 0 {
        let value = Numeric::finite(pow10(result_scale), result_scale);
        check().map_err(PowerError::Interruption)?;
        return Ok(value);
    }
    let raised = integer_power_coefficient(mantissa, magnitude, check)?;
    let raised_scale = scale.saturating_mul(magnitude);
    if exponent > 0 {
        check().map_err(PowerError::Interruption)?;
        let rounded = Numeric::finite(raised, raised_scale)
            .round_to(i32::try_from(result_scale).unwrap_or_default());
        check().map_err(PowerError::Interruption)?;
        return rounded.check_format().map_err(PowerError::TypeInput);
    }
    if raised.is_zero() {
        return Err(division_by_zero().into());
    }
    check().map_err(PowerError::Interruption)?;
    let value = divide_finite(&BigInt::one(), 0, &raised, raised_scale, result_scale);
    check().map_err(PowerError::Interruption)?;
    value.check_format().map_err(PowerError::TypeInput)
}

fn integer_power_estimate(mantissa: &BigInt, scale: u32, exponent: i32) -> f64 {
    if mantissa.is_zero() {
        return 0.0;
    }
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
}

fn integer_power_result_scale(estimate: f64, scale: u32, exponent_scale: u32) -> u32 {
    clamp_scale(
        (MIN_SIG_DIGITS - estimate.trunc().to_i64().unwrap_or_default())
            .max(i64::from(scale))
            .max(i64::from(exponent_scale)),
    )
}

fn integer_power_coefficient<E>(
    mantissa: &BigInt,
    exponent: u32,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<BigInt, PowerError<E>> {
    let mut remaining = exponent;
    let mut factor = mantissa.clone();
    let mut result = BigInt::one();
    while remaining != 0 {
        check().map_err(PowerError::Interruption)?;
        if remaining & 1 != 0 {
            result *= &factor;
            check().map_err(PowerError::Interruption)?;
        }
        remaining >>= 1;
        if remaining != 0 {
            factor = &factor * &factor;
            check().map_err(PowerError::Interruption)?;
        }
    }
    Ok(result)
}

// Limit the unreduced integer coefficient, including fractional base digits.
// Larger powers use the bounded-precision logarithm/exponential path instead.
fn integer_workspace_fits(mantissa: &BigInt, exponent: i32) -> bool {
    let limit = u64::try_from(MAX_INTEGER_DIGITS)
        .unwrap_or(u64::MAX)
        .saturating_add(u64::from(MAX_DISPLAY_SCALE))
        .saturating_mul(4);
    mantissa
        .bits()
        .saturating_mul(u64::from(exponent.unsigned_abs()))
        <= limit
}

pub(crate) fn workspace_bytes(base: &Numeric, exponent: &Numeric) -> Option<usize> {
    const SMALL_WORKSPACE_BYTES: usize = 256;
    if base.is_zero()
        || exponent.is_zero()
        || !matches!(base, Numeric::Finite { .. })
        || !matches!(exponent, Numeric::Finite { .. })
    {
        return Some(SMALL_WORKSPACE_BYTES);
    }
    if base.compare(&one()).is_eq() || base.compare(&Numeric::from_i64(-1)).is_eq() {
        return Some(SMALL_WORKSPACE_BYTES);
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
        return Some(SMALL_WORKSPACE_BYTES);
    };
    let integer_exponent = exponent.truncate_to(0);
    let integral = integer_exponent.compare(exponent).is_eq();
    let integer_power = if integral {
        exponent
            .to_i64_rounded()
            .ok()
            .flatten()
            .and_then(|value| i32::try_from(value).ok())
    } else {
        None
    };
    if let Some(power) = integer_power.filter(|power| integer_workspace_fits(base_mantissa, *power))
    {
        let estimate = integer_power_estimate(base_mantissa, *base_scale, power);
        if estimate > 131_072.0 || estimate + 1.0 < -f64::from(MAX_DISPLAY_SCALE) || power == 0 {
            return Some(SMALL_WORKSPACE_BYTES);
        }
        let result_scale = integer_power_result_scale(estimate, *base_scale, *exponent_scale);
        let exponent_magnitude = u64::from(power.unsigned_abs());
        let coefficient_bits = base_mantissa.bits().saturating_mul(exponent_magnitude);
        let scale_bits = u64::from(*base_scale)
            .saturating_mul(exponent_magnitude)
            .saturating_mul(4);
        let result_bits = u64::from(result_scale).saturating_mul(4);
        let bits = coefficient_bits.max(scale_bits).saturating_add(result_bits);
        return workspace_bytes_for_bits(bits, SMALL_WORKSPACE_BYTES);
    }

    let logarithm = logarithm_estimate(base_mantissa, *base_scale)
        * Numeric::finite(exponent_mantissa.clone(), *exponent_scale).to_f64();
    if (!integral && logarithm > f64::from(MAX_EXP_ARGUMENT) + 20.0)
        || logarithm > 131_073.0 * std::f64::consts::LN_10
        || logarithm < -(f64::from(MAX_DISPLAY_SCALE) + 2.0) * std::f64::consts::LN_10
    {
        return Some(SMALL_WORKSPACE_BYTES);
    }
    let weight = (logarithm * std::f64::consts::LOG10_E)
        .trunc()
        .to_i64()
        .unwrap_or_default();
    let result_scale = clamp_scale(
        (MIN_SIG_DIGITS - weight)
            .max(i64::from(*base_scale))
            .max(i64::from(*exponent_scale)),
    );
    let (_, exponent_weight) = Numeric::base_digits(exponent_mantissa, *exponent_scale);
    let exponent_guard =
        u32::try_from((i64::from(exponent_weight) + 1).max(0) * 4).unwrap_or_default();
    let digits =
        power_working_digits(result_scale, weight, *base_scale, exponent_guard).saturating_add(64);
    workspace_bytes_for_bits(u64::from(digits).saturating_mul(4), SMALL_WORKSPACE_BYTES)
}

/// Bounds the temporary decimal-estimation allocations needed to size a power workspace.
pub(crate) fn preflight_workspace_bytes(base: &Numeric, exponent: &Numeric) -> Option<usize> {
    fn value_bytes(value: &Numeric) -> Option<usize> {
        match value {
            Numeric::Finite { mantissa, scale } => {
                let coefficient = usize::try_from(mantissa.bits().div_ceil(8)).ok()?;
                coefficient
                    .checked_mul(16)?
                    .checked_add(usize::try_from(*scale).ok()?.checked_mul(4)?)?
                    .checked_add(256)
            }
            Numeric::NaN | Numeric::Infinity | Numeric::NegInfinity => Some(256),
        }
    }

    value_bytes(base)?
        .checked_add(value_bytes(exponent)?)?
        .checked_add(512)
}

fn workspace_bytes_for_bits(bits: u64, minimum: usize) -> Option<usize> {
    let bytes = usize::try_from(bits.saturating_add(7) / 8).ok()?;
    bytes.checked_mul(4)?.checked_add(minimum)
}

// Estimate ln(base) from leading base-10000 digits; converting the whole value
// to FLOAT8 would turn finite numeric bases into zero or infinity.
fn logarithm_estimate(mantissa: &BigInt, scale: u32) -> f64 {
    let (digits, weight) = Numeric::base_digits(mantissa, scale);
    if matches!(weight, -1 | 0) {
        let one = pow10(scale);
        let difference = mantissa - &one;
        if difference.abs() * 10u8 <= one {
            // Subtract exactly before converting. Subtracting floating logs
            // near one can amplify cancellation error by a huge exponent.
            return Numeric::finite(difference, scale).to_f64().ln_1p();
        }
    }
    let mut leading = 0.0;
    let mut divisor = 1.0;
    for digit in digits.iter().take(4) {
        leading += f64::from(*digit) / divisor;
        divisor *= 10_000.0;
    }
    leading.ln() + f64::from(weight) * 4.0 * std::f64::consts::LN_10
}
