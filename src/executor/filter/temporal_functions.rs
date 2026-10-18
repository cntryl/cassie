//! Date/time builtins over evaluated argument values.
use super::temporal::{session_of, type_error};
use super::QueryError;
use crate::types::extended::temporal::calendar::{round_time, round_timestamp, USECS_PER_DAY};
use crate::types::extended::temporal::functions as temporal;
use crate::types::extended::temporal::ops;
use crate::types::extended::temporal::zone::Zone;
use crate::types::extended::temporal::{TemporalSession, TimeTz};
use crate::types::extended::{convert_temporal, ExtendedType, TypeInputError};
use crate::types::typed::TypedValue;
use crate::types::Value;

#[path = "temporal_extract.rs"]
mod extract;
#[path = "temporal_sleep.rs"]
mod sleep;

const TIMESTAMPTZ: ExtendedType = ExtendedType::TimestampTz { precision: None };
const TIMESTAMP: ExtendedType = ExtendedType::Timestamp { precision: None };
const INTERVAL: ExtendedType = ExtendedType::Interval { typmod: -1 };

fn execution(message: impl Into<String>) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::Execution(message.into()))
}

fn argument_error(name: &str, args: &[Value]) -> QueryError {
    let types = args
        .iter()
        .map(|value| match value {
            Value::Typed(typed) => crate::types::extended::temporal_display_name(typed.kind()),
            Value::Int64(_) => "integer",
            Value::Float64(_) => "double precision",
            Value::Bool(_) => "boolean",
            _ => "unknown",
        })
        .collect::<Vec<_>>()
        .join(", ");
    execution(format!("function {name}({types}) does not exist"))
}

/// Coerces an argument to a temporal type the way implicit casts do.
fn coerce(
    value: &Value,
    kind: ExtendedType,
    session: &TemporalSession,
) -> Result<TypedValue, QueryError> {
    match value {
        Value::Typed(typed) if typed.kind() == kind.base() => Ok(typed.clone()),
        Value::Typed(typed) => convert_temporal(typed, kind, session).map_err(type_error),
        Value::String(text) => kind.parse_text_in(text, session).map_err(type_error),
        _ => Err(execution(format!(
            "cannot cast value to {}",
            crate::types::extended::temporal_display_name(kind)
        ))),
    }
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Typed(typed) => Some(typed.text()),
        _ => None,
    }
}

fn float(value: &Value) -> Option<f64> {
    match value {
        Value::Int64(value) => num_traits::ToPrimitive::to_f64(value),
        Value::Float64(value) => Some(*value),
        Value::String(text) => text.trim().parse().ok(),
        Value::Typed(typed) => typed.as_f64(),
        _ => None,
    }
}

fn integer(value: &Value) -> Result<i32, QueryError> {
    match value {
        Value::Int64(value) => i32::try_from(*value).map_err(|_| execution("integer out of range")),
        Value::Float64(value) => {
            crate::types::extended::temporal::tokens::truncate_f64_i32(value.round())
                .ok_or_else(|| execution("integer out of range"))
        }
        Value::String(text) => text
            .trim()
            .parse()
            .map_err(|_| execution(format!("invalid input syntax for type integer: \"{text}\""))),
        _ => Err(execution("integer argument required")),
    }
}

fn typed(value: TypedValue) -> Value {
    Value::Typed(value)
}

fn zone_argument(value: &Value, session: &TemporalSession) -> Result<Zone, QueryError> {
    match value {
        Value::Typed(TypedValue::Interval(interval)) => {
            temporal::zone_interval(interval).map_err(type_error)
        }
        other => {
            let name = text(other).ok_or_else(|| execution("time zone argument must be text"))?;
            temporal::zone_named(&name, session).map_err(type_error)
        }
    }
}

/// Precision argument of `current_timestamp(p)` and friends.
fn precision(args: &[Value]) -> Result<Option<u8>, QueryError> {
    args.first()
        .map(|value| {
            let precision = integer(value)?;
            if precision < 0 {
                return Err(execution("TIMESTAMP(%d) precision must not be negative"));
            }
            Ok(u8::try_from(precision.min(6)).unwrap_or(6))
        })
        .transpose()
}

fn clock(name: &str, args: &[Value], session: &TemporalSession) -> Result<Value, QueryError> {
    let now = session.transaction_start;
    Ok(match name {
        "now" | "transaction_timestamp" => typed(TypedValue::TimestampTz(now)),
        "statement_timestamp" => typed(TypedValue::TimestampTz(session.statement_start)),
        "clock_timestamp" => typed(TypedValue::TimestampTz(
            crate::types::extended::temporal::session::current_timestamp(),
        )),
        "current_timestamp" => typed(TypedValue::TimestampTz(round_timestamp(
            now,
            precision(args)?,
        ))),
        "localtimestamp" => typed(TypedValue::Timestamp(round_timestamp(
            session.zone.utc_to_local(now),
            precision(args)?,
        ))),
        "current_date" => {
            let day = session.zone.utc_to_local(now).div_euclid(USECS_PER_DAY);
            typed(TypedValue::Date(
                i32::try_from(day).map_err(|_| execution("date out of range"))?,
            ))
        }
        "localtime" => typed(TypedValue::Time(round_time(
            session.zone.utc_to_local(now).rem_euclid(USECS_PER_DAY),
            precision(args)?,
        ))),
        "current_time" => {
            let east = session.zone.offset_at(now);
            let local = session.zone.utc_to_local(now).rem_euclid(USECS_PER_DAY);
            typed(TypedValue::TimeTz(TimeTz {
                time: round_time(local, precision(args)?),
                zone: -east,
            }))
        }
        "timeofday" => Value::String(timeofday(session)),
        "pg_conf_load_time" | "pg_postmaster_start_time" => {
            static STARTED: std::sync::LazyLock<i64> = std::sync::LazyLock::new(
                crate::types::extended::temporal::session::current_timestamp,
            );
            typed(TypedValue::TimestampTz(*STARTED))
        }
        _ => return Err(execution("unknown clock")),
    })
}

fn timeofday(session: &TemporalSession) -> String {
    let now = crate::types::extended::temporal::session::current_timestamp();
    let postgres = TemporalSession {
        date_output: crate::types::extended::temporal::session::DateOutput::Postgres,
        ..session.clone()
    };
    TypedValue::TimestampTz(now).text_in(&postgres)
}

const CLOCKS: [&str; 12] = [
    "pg_conf_load_time",
    "pg_postmaster_start_time",
    "now",
    "transaction_timestamp",
    "statement_timestamp",
    "clock_timestamp",
    "current_timestamp",
    "localtimestamp",
    "current_date",
    "localtime",
    "current_time",
    "timeofday",
];

/// Evaluates a date/time builtin; `None` when `name` is not one.
pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    if !is_temporal_function(name, args) {
        return None;
    }
    let session = session_of(controls);
    if CLOCKS.contains(&name) {
        return Some(clock(name, args, &session));
    }
    if let Some(result) = sleep::evaluate(name, args, &session, controls) {
        return Some(result);
    }
    if name != "overlaps" && args.iter().any(|value| matches!(value, Value::Null)) {
        return Some(Ok(Value::Null));
    }
    Some(dispatch(name, args, &session))
}

/// Whether `name` with these arguments is a date/time builtin here.
fn is_temporal_function(name: &str, args: &[Value]) -> bool {
    match name {
        "to_char" => match args.first() {
            Some(Value::Typed(value)) => ops::is_temporal(value),
            Some(Value::Null) => true,
            _ => false,
        },
        "to_timestamp" | "to_date" | "to_number" | "date_part" | "extract" | "date_trunc"
        | "date_bin" | "age" | "justify_days" | "justify_hours" | "justify_interval"
        | "make_date" | "make_time" | "make_timestamp" | "make_timestamptz" | "make_interval"
        | "isfinite" | "overlaps" | "timezone" | "date_add" | "date_subtract" | "pg_sleep"
        | "pg_sleep_for" | "pg_sleep_until" => true,
        name => CLOCKS.contains(&name),
    }
}

/// `justify_*` and the `make_*` constructors.
fn constructor(
    name: &str,
    args: &[Value],
    session: &TemporalSession,
) -> Result<Option<Value>, QueryError> {
    let result = match (name, args) {
        ("justify_days" | "justify_hours" | "justify_interval", [value]) => {
            let TypedValue::Interval(interval) = coerce(value, INTERVAL, session)? else {
                return Err(argument_error(name, args));
            };
            let justified = match name {
                "justify_days" => interval.justify_days(),
                "justify_hours" => interval.justify_hours(),
                _ => interval.justify_interval(),
            };
            justified
                .map(|value| typed(TypedValue::Interval(value)))
                .map_err(|_| {
                    type_error(crate::types::extended::temporal::range_error(
                        "interval out of range",
                    ))
                })
        }
        ("make_date", [year, month, day]) => {
            temporal::make_date(integer(year)?, integer(month)?, integer(day)?)
                .map(|value| typed(TypedValue::Date(value)))
                .map_err(type_error)
        }
        ("make_time", [hour, minute, second]) => {
            let seconds = float(second).ok_or_else(|| argument_error(name, args))?;
            temporal::make_time(integer(hour)?, integer(minute)?, seconds)
                .map(|value| typed(TypedValue::Time(value)))
                .map_err(type_error)
        }
        (
            "make_timestamp" | "make_timestamptz",
            [year, month, day, hour, minute, second, rest @ ..],
        ) => {
            let seconds = float(second).ok_or_else(|| argument_error(name, args))?;
            let local = temporal::make_timestamp(
                (
                    integer(year)?,
                    integer(month)?,
                    integer(day)?,
                    integer(hour)?,
                    integer(minute)?,
                ),
                seconds,
            )
            .map_err(type_error)?;
            if name == "make_timestamp" {
                Ok(typed(TypedValue::Timestamp(local)))
            } else {
                let zone = match rest {
                    [zone] => zone_argument(zone, session)?,
                    _ => session.zone.clone(),
                };
                Ok(typed(TypedValue::TimestampTz(temporal::utc_of(
                    local, &zone,
                ))))
            }
        }
        ("make_interval", parts) if parts.len() <= 7 => make_interval(parts),
        _ => return Ok(None),
    };
    result.map(Some)
}

fn dispatch(name: &str, args: &[Value], session: &TemporalSession) -> Result<Value, QueryError> {
    if let Some(value) = constructor(name, args, session)? {
        return Ok(value);
    }
    match (name, args) {
        ("date_trunc", [field, source]) => date_trunc(field, source, None, session),
        ("date_trunc", [field, source, zone]) => date_trunc(field, source, Some(zone), session),
        ("date_part", [field, source]) => date_part(field, source, session),
        ("to_number", [input, template]) => {
            let (Some(input), Some(template)) = (text(input), text(template)) else {
                return Err(argument_error(name, args));
            };
            super::number_format::to_number(&input, &template)
                .map(|value| Value::Typed(TypedValue::Numeric(value)))
        }
        ("extract", [field, source]) => {
            let unit = text(field).ok_or_else(|| execution("EXTRACT field must be text"))?;
            let original = match source {
                // The Julian day counts the local calendar day.
                Value::Typed(TypedValue::TimestampTz(instant))
                    if unit.eq_ignore_ascii_case("julian") =>
                {
                    TypedValue::Timestamp(temporal::local_of(*instant, &session.zone))
                }
                Value::Typed(typed) => typed.clone(),
                other => coerce(other, TIMESTAMPTZ, session)?,
            };
            match date_part(field, source, session)? {
                Value::Float64(part) => extract::numeric_part(&unit, &original, part),
                other => Ok(other),
            }
        }
        ("date_bin", [stride, source, origin]) => date_bin(stride, source, origin, session),
        ("age", [left, right]) => age(left, right, session),
        ("age", [value]) => {
            let today = session
                .zone
                .utc_to_local(session.transaction_start)
                .div_euclid(USECS_PER_DAY);
            let midnight = Value::Typed(TypedValue::Timestamp(today * USECS_PER_DAY));
            age(&midnight, value, session)
        }
        ("to_timestamp", [seconds]) => {
            let seconds = float(seconds).ok_or_else(|| argument_error(name, args))?;
            temporal::to_timestamp_epoch(seconds)
                .map(|value| typed(TypedValue::TimestampTz(value)))
                .map_err(type_error)
        }
        ("to_timestamp" | "to_date" | "to_char", [_, _]) => {
            super::temporal_format::evaluate(name, args, session)
        }
        ("isfinite", [value]) => {
            let value = match value {
                Value::Typed(value) => value.clone(),
                other => coerce(other, TIMESTAMPTZ, session)?,
            };
            Ok(Value::Bool(ops::is_finite(&value)))
        }
        ("overlaps", [first, second, third, fourth]) => {
            overlaps([first, second, third, fourth], session)
        }
        ("timezone", [zone, value]) => timezone(&zone_argument(zone, session)?, value, session),
        ("timezone", [value]) => timezone(&session.zone, value, session),
        ("date_add" | "date_subtract", [value, span, rest @ ..]) => {
            let TypedValue::TimestampTz(instant) = coerce(value, TIMESTAMPTZ, session)? else {
                return Err(argument_error(name, args));
            };
            let TypedValue::Interval(mut span) = coerce(span, INTERVAL, session)? else {
                return Err(argument_error(name, args));
            };
            if name == "date_subtract" {
                span = span
                    .negate()
                    .map_err(|_| execution("interval out of range"))?;
            }
            let zone = match rest {
                [zone] => zone_argument(zone, session)?,
                _ => session.zone.clone(),
            };
            ops::timestamp_plus_interval(instant, &span, Some(&zone))
                .map(|value| typed(TypedValue::TimestampTz(value)))
                .map_err(type_error)
        }
        _ => Err(argument_error(name, args)),
    }
}

fn make_interval(parts: &[Value]) -> Result<Value, QueryError> {
    let mut fields = [0_i32; 6];
    for (index, slot) in fields.iter_mut().enumerate() {
        if let Some(value) = parts.get(index) {
            *slot = integer(value)?;
        }
    }
    let seconds = parts.get(6).and_then(float).unwrap_or(0.0);
    temporal::make_interval(fields, seconds)
        .map(|value| typed(TypedValue::Interval(value)))
        .map_err(type_error)
}

fn source_value(value: &Value, session: &TemporalSession) -> Result<TypedValue, QueryError> {
    match value {
        Value::Typed(TypedValue::Date(date)) => {
            convert_temporal(&TypedValue::Date(*date), TIMESTAMPTZ, session).map_err(type_error)
        }
        Value::Typed(typed) => Ok(typed.clone()),
        other => coerce(other, TIMESTAMPTZ, session),
    }
}

fn date_trunc(
    field: &Value,
    source: &Value,
    zone: Option<&Value>,
    session: &TemporalSession,
) -> Result<Value, QueryError> {
    let unit = text(field).ok_or_else(|| execution("date_trunc unit must be text"))?;
    let zone = zone.map(|zone| zone_argument(zone, session)).transpose()?;
    match source_value(source, session)? {
        TypedValue::Timestamp(value) => {
            temporal::trunc_timestamp(&unit, value).map(TypedValue::Timestamp)
        }
        TypedValue::TimestampTz(value) => {
            temporal::trunc_timestamptz(&unit, value, zone.as_ref().unwrap_or(&session.zone))
                .map(TypedValue::TimestampTz)
        }
        TypedValue::Interval(value) => {
            temporal::trunc_interval(&unit, &value).map(TypedValue::Interval)
        }
        _ => {
            return Err(execution(
                "function date_trunc does not exist for this type",
            ))
        }
    }
    .map(typed)
    .map_err(type_error)
}

fn date_part(
    field: &Value,
    source: &Value,
    session: &TemporalSession,
) -> Result<Value, QueryError> {
    let unit = text(field).ok_or_else(|| execution("date_part unit must be text"))?;
    let value = match source {
        Value::Typed(TypedValue::Date(date)) => {
            convert_temporal(&TypedValue::Date(*date), TIMESTAMP, session).map_err(type_error)?
        }
        Value::Typed(typed) => typed.clone(),
        other => coerce(other, TIMESTAMPTZ, session)?,
    };
    let part: Result<Option<f64>, TypeInputError> = match value {
        TypedValue::Timestamp(value) => {
            temporal::part_timestamp(&unit, value, None, "timestamp without time zone")
        }
        TypedValue::TimestampTz(value) => {
            let east = session.zone.offset_at(value);
            temporal::part_timestamp(
                &unit,
                temporal::local_of(value, &session.zone),
                Some(east),
                "timestamp with time zone",
            )
        }
        TypedValue::Time(time) => temporal::part_time(&unit, time, None).map(Some),
        TypedValue::TimeTz(value) => {
            temporal::part_time(&unit, value.time, Some(-value.zone)).map(Some)
        }
        TypedValue::Interval(value) => temporal::part_interval(&unit, &value),
        _ => return Err(execution("function date_part does not exist for this type")),
    };
    Ok(part
        .map_err(type_error)?
        .map_or(Value::Null, Value::Float64))
}

fn date_bin(
    stride: &Value,
    source: &Value,
    origin: &Value,
    session: &TemporalSession,
) -> Result<Value, QueryError> {
    let TypedValue::Interval(stride) = coerce(stride, INTERVAL, session)? else {
        return Err(execution("date_bin stride must be an interval"));
    };
    match source_value(source, session)? {
        TypedValue::Timestamp(value) => {
            let TypedValue::Timestamp(origin) = coerce(origin, TIMESTAMP, session)? else {
                return Err(execution("date_bin origin must be a timestamp"));
            };
            temporal::date_bin(&stride, value, origin).map(TypedValue::Timestamp)
        }
        TypedValue::TimestampTz(value) => {
            let TypedValue::TimestampTz(origin) = coerce(origin, TIMESTAMPTZ, session)? else {
                return Err(execution("date_bin origin must be a timestamp"));
            };
            temporal::date_bin(&stride, value, origin).map(TypedValue::TimestampTz)
        }
        _ => return Err(execution("function date_bin does not exist for this type")),
    }
    .map(typed)
    .map_err(type_error)
}

fn age(left: &Value, right: &Value, session: &TemporalSession) -> Result<Value, QueryError> {
    let zoned = matches!(left, Value::Typed(TypedValue::TimestampTz(_)))
        || matches!(right, Value::Typed(TypedValue::TimestampTz(_)));
    let local = |value: &Value| -> Result<i64, QueryError> {
        if zoned {
            match coerce(value, TIMESTAMPTZ, session)? {
                TypedValue::TimestampTz(value) => Ok(temporal::local_of(value, &session.zone)),
                _ => Err(execution("age requires timestamps")),
            }
        } else {
            match coerce(value, TIMESTAMP, session)? {
                TypedValue::Timestamp(value) => Ok(value),
                _ => Err(execution("age requires timestamps")),
            }
        }
    };
    temporal::age(local(left)?, local(right)?)
        .map(|value| typed(TypedValue::Interval(value)))
        .map_err(type_error)
}

fn overlaps(args: [&Value; 4], session: &TemporalSession) -> Result<Value, QueryError> {
    let kind = args
        .iter()
        .find_map(|value| match value {
            Value::Typed(TypedValue::Interval(_)) | Value::Null | Value::String(_) => None,
            Value::Typed(typed) => Some(typed.kind()),
            _ => None,
        })
        .unwrap_or(TIMESTAMPTZ);
    let point = |value: &Value| -> Result<Option<TypedValue>, QueryError> {
        match value {
            Value::Null => Ok(None),
            other => coerce(other, kind, session).map(Some),
        }
    };
    let end =
        |start: &Option<TypedValue>, value: &Value| -> Result<Option<TypedValue>, QueryError> {
            match (start, value) {
                (Some(start), Value::Typed(TypedValue::Interval(span))) => {
                    match ops::arithmetic(
                        &ops::Operand::Typed(start.clone()),
                        ops::ArithmeticOp::Add,
                        &ops::Operand::Typed(TypedValue::Interval(*span)),
                        session,
                    ) {
                        Some(Ok(ops::Outcome::Typed(value))) => Ok(Some(value)),
                        Some(Err(error)) => Err(type_error(error)),
                        _ => Err(execution("OVERLAPS endpoints must be comparable")),
                    }
                }
                (None, Value::Typed(TypedValue::Interval(_))) => Ok(None),
                (_, other) => point(other),
            }
        };
    let s1 = point(args[0])?;
    let e1 = end(&s1, args[1])?;
    let s2 = point(args[2])?;
    let e2 = end(&s2, args[3])?;
    Ok(overlap_truth(s1, e1, s2, e2, session).map_or(Value::Null, Value::Bool))
}

fn overlap_truth(
    s1: Option<TypedValue>,
    e1: Option<TypedValue>,
    s2: Option<TypedValue>,
    e2: Option<TypedValue>,
    session: &TemporalSession,
) -> Option<bool> {
    use std::cmp::Ordering::{Greater, Less};
    let cmp = |left: &TypedValue, right: &TypedValue| ops::compare(left, right, session);
    let order = |start: Option<TypedValue>, end: Option<TypedValue>| match (start, end) {
        (Some(start), Some(end)) if cmp(&start, &end) == Some(Greater) => (Some(end), Some(start)),
        (None, Some(end)) => (Some(end), None),
        pair => pair,
    };
    let (s1, e1) = order(s1, e1);
    let (s2, e2) = order(s2, e2);
    let (s1, s2) = (s1?, s2?);
    match cmp(&s1, &s2)? {
        Greater => {
            if cmp(&s1, e2.as_ref()?)? == Less {
                return Some(true);
            }
            e1.map(|_| false)
        }
        Less => {
            if cmp(&s2, e1.as_ref()?)? == Less {
                return Some(true);
            }
            e2.map(|_| false)
        }
        std::cmp::Ordering::Equal => (e1.is_some() && e2.is_some()).then_some(true),
    }
}

fn timezone(zone: &Zone, value: &Value, session: &TemporalSession) -> Result<Value, QueryError> {
    let value = match value {
        Value::Typed(typed) => typed.clone(),
        other => coerce(other, TIMESTAMPTZ, session)?,
    };
    Ok(typed(match value {
        TypedValue::Timestamp(local) => TypedValue::TimestampTz(temporal::utc_of(local, zone)),
        TypedValue::TimestampTz(instant) => {
            TypedValue::Timestamp(temporal::local_of(instant, zone))
        }
        TypedValue::Time(_) | TypedValue::TimeTz(_) => {
            let TypedValue::TimeTz(time) = coerce(
                &Value::Typed(value),
                ExtendedType::TimeTz { precision: None },
                session,
            )?
            else {
                return Err(execution("timezone requires a time with time zone"));
            };
            let east = zone.offset_at(session.transaction_start);
            let shifted =
                (time.time + i64::from(time.zone) * 1_000_000 + i64::from(east) * 1_000_000)
                    .rem_euclid(USECS_PER_DAY);
            TypedValue::TimeTz(TimeTz {
                time: shifted,
                zone: -east,
            })
        }
        TypedValue::Date(_) => {
            let TypedValue::TimestampTz(instant) =
                coerce(&Value::Typed(value), TIMESTAMPTZ, session)?
            else {
                return Err(execution("timezone requires a timestamp"));
            };
            TypedValue::Timestamp(temporal::local_of(instant, zone))
        }
        _ => return Err(execution("function timezone does not exist for this type")),
    }))
}
