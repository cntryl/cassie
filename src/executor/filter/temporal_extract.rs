//! `EXTRACT`: the `numeric` form of `date_part` with PostgreSQL's scales.
use super::QueryError;
use crate::executor::filter::temporal::type_error;
use crate::types::extended::numeric::{pow10, Numeric};
use crate::types::extended::temporal::calendar::{
    date2j, timestamp_to_fields, UNIX_TO_POSTGRES_SECS, USECS_PER_DAY, USECS_PER_SEC,
};
use crate::types::extended::temporal::tokens::{field_unit, Unit};
use crate::types::typed::TypedValue;
use crate::types::Value;

fn unsupported(unit: &str, type_name: &str) -> QueryError {
    type_error(crate::types::extended::TypeInputError::new(
        "0A000",
        format!("unit \"{unit}\" not supported for type {type_name}"),
    ))
}

fn scaled(mantissa: i128, scale: u32) -> Numeric {
    Numeric::finite(num_bigint::BigInt::from(mantissa), scale)
}

/// Converts a `date_part` result to `EXTRACT`'s exact `numeric`.
///
/// `source` is the original argument (before `date` was widened), `part`
/// the `date_part` value.
pub(super) fn numeric_part(
    unit_name: &str,
    source: &TypedValue,
    part: f64,
) -> Result<Value, QueryError> {
    let unit = field_unit(unit_name);
    let lower = unit_name.to_ascii_lowercase();
    if let TypedValue::Date(_) = source {
        if matches!(
            unit,
            Some(
                Unit::Hour
                    | Unit::Minute
                    | Unit::Second
                    | Unit::Millisecond
                    | Unit::Microsecond
                    | Unit::Timezone
                    | Unit::TimezoneHour
                    | Unit::TimezoneMinute
            )
        ) {
            return Err(unsupported(&lower, "date"));
        }
    }
    if part.is_infinite() {
        return Ok(Value::Typed(TypedValue::Numeric(if part > 0.0 {
            Numeric::Infinity
        } else {
            Numeric::NegInfinity
        })));
    }
    let numeric = match (unit, source) {
        (Some(Unit::Epoch), TypedValue::Timestamp(value) | TypedValue::TimestampTz(value)) => {
            scaled(
                i128::from(*value) + i128::from(UNIX_TO_POSTGRES_SECS) * i128::from(USECS_PER_SEC),
                6,
            )
        }
        (Some(Unit::Epoch), TypedValue::Time(value)) => scaled(i128::from(*value), 6),
        (Some(Unit::Epoch), TypedValue::TimeTz(value)) => scaled(
            i128::from(value.time) + i128::from(value.zone) * i128::from(USECS_PER_SEC),
            6,
        ),
        (Some(Unit::Julian), TypedValue::Timestamp(value)) => julian(*value)?,
        (Some(Unit::Second | Unit::Epoch), _) if !matches!(source, TypedValue::Date(_)) => {
            from_f64(part, 6)
        }
        (Some(Unit::Millisecond), _) => from_f64(part, 3),
        _ => from_f64(part, 0),
    };
    Ok(Value::Typed(TypedValue::Numeric(numeric)))
}

fn from_f64(value: f64, scale: u32) -> Numeric {
    let factor = num_traits::ToPrimitive::to_f64(&pow10(scale)).unwrap_or(1.0);
    let mantissa = (value * factor).round();
    scaled(
        num_traits::ToPrimitive::to_i128(&mantissa).unwrap_or(0),
        scale,
    )
}

/// The Julian day with its fractional day, divided exactly as `numeric`.
fn julian(local: i64) -> Result<Numeric, QueryError> {
    let fields = timestamp_to_fields(local).ok_or_else(|| {
        type_error(crate::types::extended::temporal::range_error(
            "timestamp out of range",
        ))
    })?;
    let day = date2j(fields.year, fields.month, fields.day);
    let micros = i128::from(local.rem_euclid(USECS_PER_DAY));
    let total = num_bigint::BigInt::from(i128::from(day) * i128::from(USECS_PER_DAY) + micros);
    // PostgreSQL divides at 20 fractional digits, rounding half away from zero.
    let divisor = num_bigint::BigInt::from(USECS_PER_DAY);
    let scaled_total = total * pow10(20) * 2 + &divisor;
    let mantissa = scaled_total / (divisor * 2);
    Ok(Numeric::finite(mantissa, 20))
}
