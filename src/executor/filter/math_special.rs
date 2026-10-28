//! Trigonometric, hyperbolic, error and gamma functions over `double precision`.
use super::{error, to_f64, QueryError, Value};
use crate::sql::functions::MathFunction;

fn out_of_range() -> QueryError {
    error("22003", "input is out of range")
}

/// Degree-based functions return exact results at the angles PostgreSQL
/// special-cases (`sind(30) = 0.5`).
fn degrees_sin(x: f64) -> f64 {
    let reduced = x.rem_euclid(360.0);
    match reduced {
        0.0 | 180.0 => 0.0,
        30.0 | 150.0 => 0.5,
        90.0 => 1.0,
        210.0 | 330.0 => -0.5,
        270.0 => -1.0,
        _ => reduced.to_radians().sin(),
    }
}

fn degrees_cos(x: f64) -> f64 {
    degrees_sin(x + 90.0)
}

fn degrees_tan(x: f64) -> f64 {
    let reduced = x.rem_euclid(180.0);
    match reduced {
        0.0 => 0.0,
        45.0 => 1.0,
        135.0 => -1.0,
        90.0 => f64::INFINITY,
        _ => reduced.to_radians().tan(),
    }
}

fn degrees_inverse(value: f64, inverse: fn(f64) -> f64) -> f64 {
    let degrees = inverse(value).to_degrees();
    let rounded = degrees.round();
    if (degrees - rounded).abs() < 1e-9 {
        rounded
    } else {
        degrees
    }
}

/// `erf` by its Maclaurin series near zero and the `erfc` continued fraction beyond.
fn erf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.abs() > 3.0 {
        return (1.0 - erfc_tail(x.abs())).copysign(x);
    }
    let mut term = x;
    let mut sum = x;
    let squared = x * x;
    let mut n = 0.0;
    loop {
        n += 1.0;
        term *= -squared / n;
        let step = term / (2.0 * n + 1.0);
        sum += step;
        if step.abs() < 1e-17 * sum.abs() {
            break;
        }
    }
    sum * 2.0 / std::f64::consts::PI.sqrt()
}

/// `erfc(x)` for `x > 0` by Lentz's continued fraction.
fn erfc_tail(x: f64) -> f64 {
    let mut fraction = 0.0;
    for k in (1..=60).rev() {
        fraction = f64::from(k) / 2.0 / (x + fraction);
    }
    (-x * x).exp() / std::f64::consts::PI.sqrt() / (x + fraction)
}

fn erfc(x: f64) -> f64 {
    if x > 3.0 {
        erfc_tail(x)
    } else {
        1.0 - erf(x)
    }
}

const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// `ln(Γ(x))` for `x > 0.5` by the Lanczos approximation.
fn ln_gamma_positive(x: f64) -> f64 {
    let x = x - 1.0;
    let mut sum = LANCZOS[0];
    for (index, coefficient) in LANCZOS.iter().enumerate().skip(1) {
        sum += coefficient / (x + crate::types::numeric::usize_to_f64(index));
    }
    let t = x + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + sum.ln()
}

fn gamma(x: f64) -> Result<f64, QueryError> {
    if x == 0.0 || (x < 0.0 && x.fract() == 0.0) || x == f64::NEG_INFINITY {
        return Err(out_of_range());
    }
    if x.fract() == 0.0 && x <= 171.0 {
        // Integral arguments are exact factorials.
        let mut product = 1.0f64;
        let mut factor = 2.0;
        while factor < x {
            product *= factor;
            factor += 1.0;
        }
        return Ok(product);
    }
    let value = if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma(1.0 - x)?)
    } else {
        ln_gamma_positive(x).exp()
    };
    if value.is_infinite() {
        return Err(error("22003", "value out of range: overflow"));
    }
    Ok(value)
}

fn lgamma(x: f64) -> Result<f64, QueryError> {
    if x.total_cmp(&1.0).is_eq() || x.total_cmp(&2.0).is_eq() {
        return Ok(0.0);
    }
    if x <= 0.0 && x.fract() == 0.0 {
        return Err(error("22003", "value out of range: overflow"));
    }
    if x < 0.5 {
        let reflected = (std::f64::consts::PI / (std::f64::consts::PI * x).sin())
            .abs()
            .ln();
        return Ok(reflected - lgamma(1.0 - x)?);
    }
    Ok(ln_gamma_positive(x))
}

pub(super) fn evaluate(function: MathFunction, x: f64) -> Result<f64, QueryError> {
    use MathFunction as F;
    let unit = |x: f64| (-1.0..=1.0).contains(&x) || x.is_nan();
    Ok(match function {
        F::Degrees => x.to_degrees(),
        F::Radians => x.to_radians(),
        F::Sin => x.sin(),
        F::Cos => x.cos(),
        F::Tan => x.tan(),
        F::Cot => 1.0 / x.tan(),
        F::Asin if unit(x) => x.asin(),
        F::Acos if unit(x) => x.acos(),
        F::Atan => x.atan(),
        F::Sind => degrees_sin(x),
        F::Cosd => degrees_cos(x),
        F::Tand => degrees_tan(x),
        F::Cotd => 1.0 / degrees_tan(x),
        F::Asind if unit(x) => degrees_inverse(x, f64::asin),
        F::Acosd if unit(x) => degrees_inverse(x, f64::acos),
        F::Atand => degrees_inverse(x, f64::atan),
        F::Sinh => x.sinh(),
        F::Cosh => x.cosh(),
        F::Tanh => x.tanh(),
        F::Asinh => x.asinh(),
        F::Acosh if x >= 1.0 || x.is_nan() => x.acosh(),
        F::Atanh if (-1.0..=1.0).contains(&x) || x.is_nan() => {
            if x.abs().total_cmp(&1.0).is_eq() {
                return Err(error("22003", "value out of range: overflow"));
            }
            x.atanh()
        }
        F::Erf => erf(x),
        F::Erfc => erfc(x),
        F::Gamma => gamma(x)?,
        F::Lgamma => lgamma(x)?,
        _ => return Err(out_of_range()),
    })
}

/// `width_bucket(float8, float8, float8, int)`.
pub(super) fn width_bucket_float(
    operand: f64,
    low: f64,
    high: f64,
    count: i64,
) -> Result<Value, QueryError> {
    if count <= 0 {
        return Err(error("2201G", "count must be greater than zero"));
    }
    if operand.is_nan() || low.is_nan() || high.is_nan() {
        return Err(error(
            "2201G",
            "operand, lower bound, and upper bound cannot be NaN",
        ));
    }
    if !low.is_finite() || !high.is_finite() {
        return Err(error("2201G", "lower and upper bounds must be finite"));
    }
    if low.total_cmp(&high).is_eq() {
        return Err(error("2201G", "lower bound cannot equal upper bound"));
    }
    let buckets = crate::types::numeric::i64_to_f64(count);
    let bucket = if low < high {
        if operand < low {
            return Ok(Value::Int64(0));
        }
        if operand >= high {
            return Ok(Value::Int64(count.saturating_add(1)));
        }
        ((operand - low) / (high - low) * buckets).floor() + 1.0
    } else {
        if operand > low {
            return Ok(Value::Int64(0));
        }
        if operand <= high {
            return Ok(Value::Int64(count.saturating_add(1)));
        }
        ((low - operand) / (low - high) * buckets).floor() + 1.0
    };
    Ok(Value::Int64(
        format!("{bucket:.0}")
            .parse::<i64>()
            .unwrap_or(count)
            .min(count),
    ))
}

/// `width_bucket(anycompatible, anycompatiblearray)` over sorted thresholds.
pub(super) fn width_bucket_array(operand: &Value, thresholds: &Value) -> Result<Value, QueryError> {
    if matches!(operand, Value::Null) || matches!(thresholds, Value::Null) {
        return Ok(Value::Null);
    }
    if let Value::Array(array) = thresholds {
        if array.dimensions().len() > 1 {
            return Err(error("2202E", "thresholds must be one-dimensional array"));
        }
        if array
            .elements()
            .iter()
            .any(|value| matches!(value, Value::Null))
        {
            return Err(error("22004", "thresholds array must not contain NULLs"));
        }
        let position = array.elements().partition_point(|bound| {
            crate::types::semantic::compare_values(bound, operand) != std::cmp::Ordering::Greater
        });
        return Ok(Value::Int64(
            i64::try_from(position).map_err(|_| out_of_range())?,
        ));
    }
    let operand = to_f64(operand)?;
    let bounds = match thresholds {
        Value::Json(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| item.as_f64().ok_or_else(out_of_range))
            .collect::<Result<Vec<_>, _>>()?,
        Value::String(text) => text
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split(',')
            .filter(|item| !item.trim().is_empty())
            .map(|item| item.trim().parse::<f64>().map_err(|_| out_of_range()))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(out_of_range()),
    };
    let position = bounds.partition_point(|bound| *bound <= operand);
    Ok(Value::Int64(i64::try_from(position).unwrap_or(i64::MAX)))
}
