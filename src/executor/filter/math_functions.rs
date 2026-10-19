//! PostgreSQL 18 mathematical functions over `numeric` and `double precision`.
//!
//! A `numeric` argument selects the exact overload; integers and floats
//! select the `double precision` overload, as PostgreSQL's resolution does.
use super::{QueryError, Value};
use crate::app::CassieError;
use crate::sql::functions::MathFunction;
use crate::types::extended::numeric::{math, parse_text, Numeric};
use crate::types::extended::TypeInputError;
use crate::types::typed::TypedValue;

#[path = "math_special.rs"]
mod special;

fn input_error(error: TypeInputError) -> QueryError {
    if error.sqlstate == "22012" {
        return QueryError::General("division by zero".to_string());
    }
    QueryError::Cassie(CassieError::TypeInput(error))
}

fn error(sqlstate: &'static str, message: &str) -> QueryError {
    input_error(TypeInputError {
        sqlstate,
        message: message.to_string(),
    })
}

fn numeric_value(value: Numeric) -> Value {
    Value::Typed(TypedValue::Numeric(value))
}

fn is_numeric(value: &Value) -> bool {
    matches!(value, Value::Typed(TypedValue::Numeric(_)))
}

fn to_numeric(value: &Value) -> Result<Numeric, QueryError> {
    match value {
        Value::Typed(TypedValue::Numeric(value)) => Ok(value.clone()),
        Value::Typed(TypedValue::Real(value)) => Numeric::from_f32(*value).map_err(input_error),
        Value::Int64(value) => Ok(Numeric::from_i64(*value)),
        Value::Float64(value) => Numeric::from_f64(*value).map_err(input_error),
        Value::String(text) => parse_text(text).map_err(input_error),
        _ => Err(undefined("numeric")),
    }
}

fn to_f64(value: &Value) -> Result<f64, QueryError> {
    match value {
        Value::Float64(value) => Ok(*value),
        Value::Int64(value) => Ok(crate::types::numeric::i64_to_f64(*value)),
        Value::Typed(typed) => typed.as_f64().ok_or_else(|| undefined("double precision")),
        Value::String(text) => crate::types::float_input::parse(text)
            .map_err(|error| QueryError::Cassie(CassieError::FloatInput(error))),
        _ => Err(undefined("double precision")),
    }
}

fn to_i64(value: &Value) -> Result<i64, QueryError> {
    match value {
        Value::Int64(value) => Ok(*value),
        Value::String(text) => text.trim().parse::<i64>().map_err(|_| {
            error(
                "22P02",
                &format!("invalid input syntax for type integer: \"{text}\""),
            )
        }),
        Value::Typed(TypedValue::Numeric(value)) => value
            .to_i64_rounded()
            .map_err(input_error)?
            .ok_or_else(|| error("22003", "integer out of range")),
        Value::Float64(value) if value.is_finite() => format!("{:.0}", value.round_ties_even())
            .parse::<i64>()
            .map_err(|_| error("22003", "integer out of range")),
        _ => Err(undefined("integer")),
    }
}

fn undefined(kind: &str) -> QueryError {
    QueryError::Cassie(CassieError::Execution(format!(
        "function argument does not match {kind}"
    )))
}

fn float_range(value: f64, inputs_finite: bool) -> Result<Value, QueryError> {
    if inputs_finite && value.is_infinite() {
        return Err(error("22003", "value out of range: overflow"));
    }
    Ok(Value::Float64(value))
}

/// Evaluates `name` when it is a math function; `None` defers to other families.
pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let metadata = crate::sql::functions::function(name)?;
    let crate::sql::functions::FunctionId::Math(function) = metadata.id else {
        if matches!(name, "power" | "pow") && args.iter().any(is_numeric) {
            return Some(binary_numeric(args, math::power));
        }
        return None;
    };
    if args.iter().any(|value| matches!(value, Value::Null))
        && !matches!(function, MathFunction::WidthBucket)
    {
        return Some(Ok(Value::Null));
    }
    Some(dispatch(function, args))
}

fn binary_numeric(
    args: &[Value],
    operation: fn(&Numeric, &Numeric) -> Result<Numeric, TypeInputError>,
) -> Result<Value, QueryError> {
    let [left, right] = args else {
        return Err(undefined("numeric"));
    };
    if matches!(left, Value::Null) || matches!(right, Value::Null) {
        return Ok(Value::Null);
    }
    operation(&to_numeric(left)?, &to_numeric(right)?)
        .map(numeric_value)
        .map_err(input_error)
}

fn dispatch(function: MathFunction, args: &[Value]) -> Result<Value, QueryError> {
    use MathFunction as F;
    let first = args.first().unwrap_or(&Value::Null);
    match function {
        F::Round | F::Trunc if args.len() == 2 => {
            let value = to_numeric(first)?;
            let scale = to_i64(&args[1])?;
            if function == F::Round {
                math::round(&value, scale)
                    .map(numeric_value)
                    .map_err(input_error)
            } else {
                Ok(numeric_value(math::trunc(&value, scale)))
            }
        }
        F::Log if args.len() == 2 => {
            if args
                .iter()
                .any(|value| matches!(value, Value::Float64(_) | Value::Typed(TypedValue::Real(_))))
            {
                return Err(QueryError::Cassie(CassieError::Execution(
                    "function log(double precision, double precision) does not exist".into(),
                )));
            }
            binary_numeric(args, math::log)
        }
        F::Round
        | F::Trunc
        | F::Ceil
        | F::Ceiling
        | F::Floor
        | F::Sign
        | F::Sqrt
        | F::Exp
        | F::Ln
        | F::Log
        | F::Log10
            if is_numeric(first) =>
        {
            numeric_unary(function, &to_numeric(first)?)
        }
        F::Mod | F::Gcd | F::Lcm if args.iter().all(|value| matches!(value, Value::Int64(_))) => {
            integer_binary(function, to_i64(first)?, to_i64(&args[1])?)
        }
        F::Mod => binary_numeric(args, math::modulo),
        F::Div => binary_numeric(args, math::divide_truncated),
        F::Gcd | F::Lcm => Ok(numeric_gcd(
            function,
            &to_numeric(first)?,
            &to_numeric(&args[1])?,
        )),
        F::Scale => Ok(to_numeric(first)?
            .scale()
            .map_or(Value::Null, |scale| Value::Int64(i64::from(scale)))),
        F::MinScale => Ok(math::min_scale(&to_numeric(first)?)
            .map_or(Value::Null, |scale| Value::Int64(i64::from(scale)))),
        F::TrimScale => Ok(numeric_value(math::trim_scale(&to_numeric(first)?))),
        F::Factorial => math::factorial(to_i64(first)?)
            .map(numeric_value)
            .map_err(input_error),
        F::WidthBucket => width_bucket(args),
        F::Random | F::RandomNormal | F::Setseed => {
            Err(QueryError::Cassie(CassieError::Unsupported(format!(
                "{} requires a session random state",
                format!("{function:?}").to_ascii_lowercase()
            ))))
        }
        _ => float_function(function, args),
    }
}

fn numeric_unary(function: MathFunction, value: &Numeric) -> Result<Value, QueryError> {
    use MathFunction as F;
    let result = match function {
        F::Round => math::round(value, 0),
        F::Trunc => Ok(math::trunc(value, 0)),
        F::Ceil | F::Ceiling => Ok(math::ceil(value)),
        F::Floor => Ok(math::floor(value)),
        F::Sign => Ok(math::sign(value)),
        F::Sqrt => math::sqrt(value),
        F::Exp => math::exp(value),
        F::Ln => math::ln(value),
        _ => math::log(&Numeric::from_i64(10), value),
    };
    result.map(numeric_value).map_err(input_error)
}

fn integer_binary(function: MathFunction, left: i64, right: i64) -> Result<Value, QueryError> {
    match function {
        MathFunction::Mod => {
            if right == 0 {
                return Err(QueryError::General("division by zero".to_string()));
            }
            Ok(Value::Int64(left.checked_rem(right).unwrap_or(0)))
        }
        MathFunction::Gcd => {
            let gcd = num_integer::gcd(i128::from(left), i128::from(right));
            i64::try_from(gcd)
                .map(Value::Int64)
                .map_err(|_| error("22003", "bigint out of range"))
        }
        _ => {
            let lcm = num_integer::lcm(i128::from(left), i128::from(right)).abs();
            i64::try_from(lcm)
                .map(Value::Int64)
                .map_err(|_| error("22003", "bigint out of range"))
        }
    }
}

fn numeric_gcd(function: MathFunction, left: &Numeric, right: &Numeric) -> Value {
    let (
        Numeric::Finite {
            mantissa: a,
            scale: a_scale,
        },
        Numeric::Finite {
            mantissa: b,
            scale: b_scale,
        },
    ) = (left, right)
    else {
        return numeric_value(Numeric::NaN);
    };
    let scale = (*a_scale).max(*b_scale);
    let a = Numeric::rescaled_mantissa(a, *a_scale, scale);
    let b = Numeric::rescaled_mantissa(b, *b_scale, scale);
    let result = if function == MathFunction::Gcd {
        num_integer::Integer::gcd(&a, &b)
    } else {
        num_integer::Integer::lcm(&a, &b)
    };
    numeric_value(Numeric::finite(result, scale))
}

fn width_bucket(args: &[Value]) -> Result<Value, QueryError> {
    if let [operand, thresholds] = args {
        return special::width_bucket_array(operand, thresholds);
    }
    let [operand, low, high, count] = args else {
        return Err(undefined("width_bucket"));
    };
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let count = to_i64(count)?;
    if [operand, low, high].into_iter().any(is_numeric)
        || [operand, low, high]
            .into_iter()
            .all(|value| matches!(value, Value::Int64(_)))
    {
        return math::width_bucket(
            &to_numeric(operand)?,
            &to_numeric(low)?,
            &to_numeric(high)?,
            count,
        )
        .map(Value::Int64)
        .map_err(input_error);
    }
    special::width_bucket_float(to_f64(operand)?, to_f64(low)?, to_f64(high)?, count)
}

fn float_function(function: MathFunction, args: &[Value]) -> Result<Value, QueryError> {
    use MathFunction as F;
    if function == F::Pi {
        return Ok(Value::Float64(std::f64::consts::PI));
    }
    let x = to_f64(args.first().unwrap_or(&Value::Null))?;
    let finite = x.is_finite();
    let value = match function {
        F::Round => x.round_ties_even(),
        F::Trunc => x.trunc(),
        F::Ceil | F::Ceiling => x.ceil(),
        F::Floor => x.floor(),
        F::Sign => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        F::Sqrt => {
            if x < 0.0 {
                return Err(error(
                    "2201F",
                    "cannot take square root of a negative number",
                ));
            }
            x.sqrt()
        }
        F::Cbrt => x.cbrt(),
        F::Exp => {
            let value = x.exp();
            if finite && value == 0.0 {
                return Err(error("22003", "value out of range: underflow"));
            }
            value
        }
        F::Ln | F::Log | F::Log10 => {
            if x == 0.0 {
                return Err(error("2201E", "cannot take logarithm of zero"));
            }
            if x < 0.0 {
                return Err(error("2201E", "cannot take logarithm of a negative number"));
            }
            if function == F::Ln {
                x.ln()
            } else {
                x.log10()
            }
        }
        F::Atan2 | F::Atan2d => {
            let y = x;
            let x = to_f64(&args[1])?;
            let radians = y.atan2(x);
            if function == F::Atan2 {
                radians
            } else {
                radians.to_degrees()
            }
        }
        _ => return special::evaluate(function, x).map(Value::Float64),
    };
    float_range(value, finite)
}
