//! Exact numeric powers, with bounded integer coefficients and logarithm estimates.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::super::{one, pow10, Numeric, TypeInputError, MAX_DISPLAY_SCALE, MAX_INTEGER_DIGITS};
use super::{
    abs, approximate, clamp_scale, divide_finite, division_by_zero, error, exp_fixed, ln_fixed,
    working_digits, Fixed, MIN_SIG_DIGITS,
};

/// `power(numeric, numeric)` / `^`.
///
/// # Errors
/// Returns PostgreSQL's errors for undefined or complex powers and overflow.
pub fn power(base: &Numeric, exponent: &Numeric) -> Result<Numeric, TypeInputError> {
    if base.is_nan() || exponent.is_nan() {
        if base.compare(&one()).is_eq() || exponent.is_zero() {
            return Ok(one());
        }
        return Ok(Numeric::NaN);
    }
    let integer_exponent = exponent.truncate_to(0);
    let integral = integer_exponent.compare(exponent).is_eq();
    if base.is_zero() && exponent.signum() == Some(-1) {
        return Err(error(
            "2201F",
            "zero raised to a negative power is undefined",
        ));
    }
    if base.signum() == Some(-1) && !integral {
        return Err(error(
            "2201F",
            "a negative number raised to a non-integer power yields a complex result",
        ));
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
    if integral {
        if let Some(power) = exponent
            .to_i64_rounded()
            .ok()
            .flatten()
            .and_then(|value| i32::try_from(value).ok())
            .filter(|power| integer_workspace_fits(base_mantissa, *power))
        {
            return power_integer(base_mantissa, *base_scale, power, *exponent_scale);
        }
    }
    if base_mantissa.is_zero() {
        return Ok(Numeric::finite(BigInt::zero(), 16));
    }
    // An integral exponent outside the i32 fast path still permits a negative
    // base. Evaluate its positive magnitude and restore the exact integer sign.
    let magnitude = base_mantissa.abs();
    let ln_estimate = logarithm_estimate(&magnitude, *base_scale) * approximate(exponent);
    // A one-decimal margin keeps the approximation from rejecting a valid
    // boundary value; the exact coefficient is checked after evaluation.
    if ln_estimate > 131_073.0 * std::f64::consts::LN_10 {
        return Err(error("22003", "value overflows numeric format"));
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
            .max(i64::from(*base_scale))
            .max(i64::from(*exponent_scale)),
    );
    // Logarithm error is multiplied by the exponent. Fixed guard digits alone
    // lose published digits for bases close to one and large finite exponents.
    let (_, exponent_weight) = Numeric::base_digits(exponent_mantissa, *exponent_scale);
    let exponent_guard =
        u32::try_from((i64::from(exponent_weight) + 1).max(0) * 4).unwrap_or_default();
    let work = Fixed {
        value: BigInt::zero(),
        // A finite tiny base must remain nonzero at working precision. It may
        // have more fractional digits than the result's display-scale cap.
        digits: working_digits(result_scale, weight.abs() * 2 + i64::from(exponent_guard))
            .max(base_scale.saturating_add(exponent_guard).saturating_add(20)),
    };
    let ln_base = ln_fixed(
        &work,
        &Fixed::from_numeric(&magnitude, *base_scale, work.digits).value,
    );
    let exponent_fixed = Fixed::from_numeric(exponent_mantissa, *exponent_scale, work.digits);
    let product = work.multiply(&ln_base, &exponent_fixed.value);
    if product.is_zero() {
        let one = pow10(result_scale);
        return Ok(Numeric::finite(
            if base_mantissa.is_negative() && odd {
                -one
            } else {
                one
            },
            result_scale,
        ));
    }
    // A rounded estimate near one can miss a large exponent's effect. Bound
    // the actual fixed-point product before repeated exponential squaring.
    let ln_ten = ln_fixed(&work, &(work.one() * 10u8));
    if product > &ln_ten * MAX_INTEGER_DIGITS {
        return Err(error("22003", "value overflows numeric format"));
    }
    if product < -(&ln_ten * (MAX_DISPLAY_SCALE + 2)) {
        return Ok(Numeric::finite(BigInt::zero(), MAX_DISPLAY_SCALE));
    }
    let value = exp_fixed(&work, &product);
    Fixed {
        value: if base_mantissa.is_negative() && odd {
            -value
        } else {
            value
        },
        digits: work.digits,
    }
    .into_numeric(result_scale)
    .check_format()
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
fn power_integer(
    mantissa: &BigInt,
    scale: u32,
    exponent: i32,
    exponent_scale: u32,
) -> Result<Numeric, TypeInputError> {
    if mantissa.is_zero() {
        if exponent < 0 {
            return Err(division_by_zero());
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
    divide_finite(&BigInt::one(), 0, &raised, raised_scale, result_scale).check_format()
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

// Estimate ln(base) from leading base-10000 digits; converting the whole value
// to FLOAT8 would turn finite numeric bases into zero or infinity.
fn logarithm_estimate(mantissa: &BigInt, scale: u32) -> f64 {
    let (digits, weight) = Numeric::base_digits(mantissa, scale);
    let mut leading = 0.0;
    let mut divisor = 1.0;
    for digit in digits.iter().take(4) {
        leading += f64::from(*digit) / divisor;
        divisor *= 10_000.0;
    }
    leading.ln() + f64::from(weight) * 4.0 * std::f64::consts::LN_10
}
