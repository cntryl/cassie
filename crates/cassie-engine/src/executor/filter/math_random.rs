//! `random`, `random_normal` and `setseed` over the session's sequence.
use super::{CassieSession, QueryError, Value};
use crate::app::{CassieError, SessionRandom};
use crate::types::extended::numeric::Numeric;
use crate::types::extended::TypeInputError;
use crate::types::typed::TypedValue;
use num_bigint::BigInt;
use num_traits::Zero;

fn invalid_parameter(message: &str) -> QueryError {
    QueryError::Cassie(CassieError::TypeInput(TypeInputError {
        sqlstate: "22023",
        message: message.to_string(),
    }))
}

fn with_random<T>(
    session: Option<&CassieSession>,
    operation: impl FnOnce(&mut SessionRandom) -> T,
) -> T {
    match session {
        Some(session) => session.with_random(operation),
        None => operation(&mut SessionRandom::from_entropy()),
    }
}

fn float(value: &Value) -> Result<f64, QueryError> {
    match value {
        Value::Float64(value) => Ok(*value),
        Value::Int64(value) => Ok(crate::types::numeric::i64_to_f64(*value)),
        Value::Typed(typed) => typed
            .as_f64()
            .ok_or_else(|| invalid_parameter("random_normal requires double precision")),
        _ => Err(invalid_parameter("random_normal requires double precision")),
    }
}

fn numeric(value: &Value) -> Result<Numeric, QueryError> {
    match value {
        Value::Typed(TypedValue::Numeric(value)) => Ok(value.clone()),
        Value::Int64(value) => Ok(Numeric::from_i64(*value)),
        Value::Float64(value) => Numeric::from_f64(*value)
            .map_err(|error| QueryError::Cassie(CassieError::TypeInput(error))),
        _ => Err(invalid_parameter("random requires numeric bounds")),
    }
}

/// A uniform integer in `[0, bound)` for a positive `bound`.
fn below(random: &mut SessionRandom, bound: &BigInt) -> BigInt {
    let words = bound.bits().div_ceil(64) + 1;
    let mut value = BigInt::zero();
    for _ in 0..words {
        value = (value << 64) + random.next_u64();
    }
    value % bound
}

fn random_between(
    session: Option<&CassieSession>,
    low: &Value,
    high: &Value,
) -> Result<Value, QueryError> {
    if let (Value::Int64(low), Value::Int64(high)) = (low, high) {
        if low > high {
            return Err(invalid_parameter(
                "lower bound must be less than or equal to upper bound",
            ));
        }
        let span = BigInt::from(*high) - BigInt::from(*low) + 1;
        let offset = with_random(session, |random| below(random, &span));
        let value = BigInt::from(*low) + offset;
        return i64::try_from(value)
            .map(Value::Int64)
            .map_err(|_| invalid_parameter("random bound out of range"));
    }
    let (low, high) = (numeric(low)?, numeric(high)?);
    let (
        Numeric::Finite {
            mantissa: low_mantissa,
            scale: low_scale,
        },
        Numeric::Finite {
            mantissa: high_mantissa,
            scale: high_scale,
        },
    ) = (&low, &high)
    else {
        return Err(invalid_parameter("lower and upper bounds must be finite"));
    };
    let scale = (*low_scale).max(*high_scale);
    let low_value = Numeric::rescaled_mantissa(low_mantissa, *low_scale, scale);
    let high_value = Numeric::rescaled_mantissa(high_mantissa, *high_scale, scale);
    if low_value > high_value {
        return Err(invalid_parameter(
            "lower bound must be less than or equal to upper bound",
        ));
    }
    let span = &high_value - &low_value + 1;
    let offset = with_random(session, |random| below(random, &span));
    Ok(Value::Typed(TypedValue::Numeric(Numeric::finite(
        low_value + offset,
        scale,
    ))))
}

/// Box–Muller: one normal deviate from two uniform draws.
fn normal(random: &mut SessionRandom) -> f64 {
    let first = 1.0 - random.next_f64();
    let second = random.next_f64();
    (-2.0 * first.ln()).sqrt() * (2.0 * std::f64::consts::PI * second).cos()
}

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    if !matches!(name, "random" | "random_normal" | "setseed") {
        return None;
    }
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Some(Ok(Value::Null));
    }
    Some(match (name, args) {
        ("random", []) => Ok(Value::Float64(with_random(
            session,
            SessionRandom::next_f64,
        ))),
        ("random", [low, high]) => random_between(session, low, high),
        ("random_normal", _) if args.len() <= 2 => {
            let mean = args.first().map_or(Ok(0.0), float);
            let deviation = args.get(1).map_or(Ok(1.0), float);
            mean.and_then(|mean| {
                deviation.map(|deviation| {
                    Value::Float64(mean + deviation * with_random(session, normal))
                })
            })
        }
        ("setseed", [seed]) => float(seed).and_then(|seed| {
            if !(-1.0..=1.0).contains(&seed) {
                return Err(invalid_parameter(&format!(
                    "setseed parameter {seed} is out of allowed range [-1,1]"
                )));
            }
            with_random(session, |random| random.reseed(seed));
            Ok(Value::Null)
        }),
        _ => Err(invalid_parameter(
            "random has no overload for these arguments",
        )),
    })
}
