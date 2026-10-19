//! Catalog identity, typmods and conversions of the temporal extended types.
use super::temporal::calendar::{
    round_time, round_timestamp, DATE_NOBEGIN, DATE_NOEND, POSTGRES_EPOCH_JDATE,
    TIMESTAMP_END_JULIAN, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND, USECS_PER_DAY, USECS_PER_SEC,
};
use super::temporal::interval::interval_typmod;
use super::temporal::{self, Interval, TemporalSession, TimeTz};
use super::{ExtendedFamily, ExtendedType, TypeInputError};
use crate::types::typed::TypedValue;

/// Whether `kind` is one of the temporal types.
pub(super) const fn is_temporal(kind: ExtendedType) -> bool {
    matches!(
        kind,
        ExtendedType::Date
            | ExtendedType::Time { .. }
            | ExtendedType::TimeTz { .. }
            | ExtendedType::Timestamp { .. }
            | ExtendedType::TimestampTz { .. }
            | ExtendedType::Interval { .. }
    )
}

pub(super) const fn name(kind: ExtendedType) -> &'static str {
    match kind {
        ExtendedType::Date => "date",
        ExtendedType::Time { .. } => "time",
        ExtendedType::TimeTz { .. } => "time with time zone",
        ExtendedType::Timestamp { .. } => "timestamp",
        ExtendedType::TimestampTz { .. } => "timestamp with time zone",
        _ => "interval",
    }
}

pub(super) const fn typname(kind: ExtendedType) -> &'static str {
    match kind {
        ExtendedType::Date => "date",
        ExtendedType::Time { .. } => "time",
        ExtendedType::TimeTz { .. } => "timetz",
        ExtendedType::Timestamp { .. } => "timestamp",
        ExtendedType::TimestampTz { .. } => "timestamptz",
        _ => "interval",
    }
}

pub(super) const fn type_oid(kind: ExtendedType) -> u32 {
    match kind {
        ExtendedType::Date => 1082,
        ExtendedType::Time { .. } => 1083,
        ExtendedType::TimeTz { .. } => 1266,
        ExtendedType::Timestamp { .. } => 1114,
        ExtendedType::TimestampTz { .. } => 1184,
        _ => 1186,
    }
}

pub(super) const fn typlen(kind: ExtendedType) -> i16 {
    match kind {
        ExtendedType::Date => 4,
        ExtendedType::Time { .. }
        | ExtendedType::Timestamp { .. }
        | ExtendedType::TimestampTz { .. } => 8,
        ExtendedType::TimeTz { .. } => 12,
        _ => 16,
    }
}

pub(super) const fn category(kind: ExtendedType) -> char {
    match kind {
        ExtendedType::Interval { .. } => 'T',
        _ => 'D',
    }
}

pub(super) const fn family(kind: ExtendedType) -> ExtendedFamily {
    match kind {
        ExtendedType::Interval { .. } => ExtendedFamily::Timespan,
        _ => ExtendedFamily::Temporal,
    }
}

fn precision_typmod(precision: Option<u8>) -> i32 {
    precision.map_or(-1, i32::from)
}

pub(super) fn typmod(kind: ExtendedType) -> i32 {
    match kind {
        ExtendedType::Time { precision }
        | ExtendedType::TimeTz { precision }
        | ExtendedType::Timestamp { precision }
        | ExtendedType::TimestampTz { precision } => precision_typmod(precision),
        ExtendedType::Interval { typmod } => typmod,
        _ => -1,
    }
}

pub(super) const fn base(kind: ExtendedType) -> ExtendedType {
    match kind {
        ExtendedType::Time { .. } => ExtendedType::Time { precision: None },
        ExtendedType::TimeTz { .. } => ExtendedType::TimeTz { precision: None },
        ExtendedType::Timestamp { .. } => ExtendedType::Timestamp { precision: None },
        ExtendedType::TimestampTz { .. } => ExtendedType::TimestampTz { precision: None },
        ExtendedType::Interval { .. } => ExtendedType::Interval { typmod: -1 },
        other => other,
    }
}

pub(super) fn from_sql_name(name: &str) -> Option<ExtendedType> {
    match name {
        "timestamptz" | "timestamp with time zone" => {
            Some(ExtendedType::TimestampTz { precision: None })
        }
        "timetz" | "time with time zone" => Some(ExtendedType::TimeTz { precision: None }),
        "interval" => Some(ExtendedType::Interval { typmod: -1 }),
        _ => None,
    }
}

/// Parses a lower-cased temporal type spelling with a typmod, such as
/// `timestamp(3) with time zone`, `time(2)` or `interval day to second(3)`.
/// Plain `date`, `time` and `timestamp` stay with the existing schema types.
///
/// # Errors
/// Returns PostgreSQL's message for an invalid precision.
#[must_use]
pub fn parse_type_name(lower: &str) -> Option<Result<crate::types::DataType, String>> {
    let lower = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = lower.replace(" (", "(");
    let (with_zone, without_zone) = (
        lower.ends_with(" with time zone"),
        lower.ends_with(" without time zone"),
    );
    let base = lower
        .strip_suffix(" with time zone")
        .or_else(|| lower.strip_suffix(" without time zone"))
        .unwrap_or(&lower);
    let name = base.split_once('(').map_or(base, |(head, _)| head).trim();
    if !matches!(name, "timestamp" | "timestamptz" | "time" | "timetz") {
        return parse_interval_type(&lower);
    }
    let (head, precision) = match split_precision(base) {
        Ok(parts) => parts,
        Err(error) => return Some(Err(error)),
    };
    let extended = |kind| Some(Ok(crate::types::DataType::Extended(kind)));
    match head {
        "timestamptz" if !with_zone && !without_zone => {
            extended(ExtendedType::TimestampTz { precision })
        }
        "timetz" if !with_zone && !without_zone => extended(ExtendedType::TimeTz { precision }),
        "timestamp" if with_zone => extended(ExtendedType::TimestampTz { precision }),
        "time" if with_zone => extended(ExtendedType::TimeTz { precision }),
        "timestamp" if precision.is_some() => extended(ExtendedType::Timestamp { precision }),
        "time" if precision.is_some() => extended(ExtendedType::Time { precision }),
        _ => parse_interval_type(&lower),
    }
}

fn split_precision(base: &str) -> Result<(&str, Option<u8>), String> {
    let Some((head, rest)) = base.split_once('(') else {
        return Ok((base.trim(), None));
    };
    let Some(digits) = rest.strip_suffix(')') else {
        return Ok((base, None));
    };
    let precision = digits
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("invalid type modifier '{base}'"))?;
    if precision < 0 {
        return Err(format!(
            "{} precision must not be negative",
            head.trim().to_ascii_uppercase()
        ));
    }
    Ok((
        head.trim(),
        Some(u8::try_from(precision.min(6)).unwrap_or(6)),
    ))
}

fn parse_interval_type(lower: &str) -> Option<Result<crate::types::DataType, String>> {
    let rest = lower.strip_prefix("interval")?;
    if !rest.is_empty() && !rest.starts_with([' ', '(']) {
        return None;
    }
    let rest = rest.trim();
    let (precision, qualifier) = if let Some(after) = rest.strip_prefix('(') {
        let (digits, tail) = after.split_once(')')?;
        (
            digits.trim().parse::<u8>().ok().map(|value| value.min(6)),
            tail.trim(),
        )
    } else {
        (None, rest)
    };
    let (qualifier, precision) = match qualifier.rsplit_once("second(") {
        Some((head, digits)) => (
            format!("{head}second"),
            digits
                .trim_end_matches(')')
                .trim()
                .parse::<u8>()
                .ok()
                .map(|value| value.min(6)),
        ),
        None => (qualifier.to_string(), precision),
    };
    let range = temporal::interval::interval_range(&qualifier)?;
    let typmod = if range == temporal::interval::FULL_RANGE && precision.is_none() {
        -1
    } else {
        interval_typmod(range, precision)
    };
    Some(Ok(crate::types::DataType::Extended(
        ExtendedType::Interval { typmod },
    )))
}

pub(super) fn parse_text(
    kind: ExtendedType,
    text: &str,
    session: &TemporalSession,
) -> Result<TypedValue, TypeInputError> {
    Ok(match kind {
        ExtendedType::Date => TypedValue::Date(temporal::parse_date(text, session)?),
        ExtendedType::Time { precision } => {
            TypedValue::Time(temporal::parse_time(text, session, precision)?)
        }
        ExtendedType::TimeTz { precision } => {
            TypedValue::TimeTz(temporal::parse_timetz(text, session, precision)?)
        }
        ExtendedType::Timestamp { precision } => {
            TypedValue::Timestamp(temporal::parse_timestamp(text, session, precision)?)
        }
        ExtendedType::TimestampTz { precision } => {
            TypedValue::TimestampTz(temporal::parse_timestamptz(text, session, precision)?)
        }
        ExtendedType::Interval { typmod } => {
            TypedValue::Interval(temporal::parse_interval(text, typmod, session)?)
        }
        _ => return Err(cannot_cast("text", kind)),
    })
}

fn cannot_cast(source: &str, target: ExtendedType) -> TypeInputError {
    TypeInputError {
        sqlstate: "42846",
        message: format!("cannot cast type {source} to {}", display_name(target)),
    }
}

/// PostgreSQL's `format_type` spelling used in cast errors.
#[must_use]
pub const fn display_name(kind: ExtendedType) -> &'static str {
    match kind {
        ExtendedType::Time { .. } => "time without time zone",
        ExtendedType::Timestamp { .. } => "timestamp without time zone",
        other => other.name(),
    }
}

/// `CAST` into a temporal type; `None` for non-temporal targets.
pub(super) fn cast_from(
    kind: ExtendedType,
    value: &crate::types::Value,
    session: &TemporalSession,
) -> Option<Result<TypedValue, TypeInputError>> {
    if !is_temporal(kind) {
        return None;
    }
    Some(match value {
        crate::types::Value::String(text) => parse_text(kind, text, session),
        crate::types::Value::Typed(typed) => convert(typed, kind, session),
        crate::types::Value::Int64(_) => Err(cannot_cast("integer", kind)),
        crate::types::Value::Float64(_) => Err(cannot_cast("double precision", kind)),
        crate::types::Value::Bool(_) => Err(cannot_cast("boolean", kind)),
        _ => Err(cannot_cast("unknown", kind)),
    })
}

fn date_to_timestamp(date: i32) -> Result<i64, TypeInputError> {
    match date {
        DATE_NOBEGIN => Ok(TIMESTAMP_NOBEGIN),
        DATE_NOEND => Ok(TIMESTAMP_NOEND),
        date if i64::from(date) >= TIMESTAMP_END_JULIAN - i64::from(POSTGRES_EPOCH_JDATE) => {
            Err(temporal::range_error("date out of range for timestamp"))
        }
        date => Ok(i64::from(date) * USECS_PER_DAY),
    }
}

/// `timestamp2date`-style day of a (local) timestamp.
fn timestamp_to_date(value: i64) -> Result<i32, TypeInputError> {
    match value {
        TIMESTAMP_NOBEGIN => Ok(DATE_NOBEGIN),
        TIMESTAMP_NOEND => Ok(DATE_NOEND),
        value => i32::try_from(value.div_euclid(USECS_PER_DAY))
            .map_err(|_| temporal::range_error("date out of range")),
    }
}

/// Local wall time of an instant in the session zone.
#[must_use]
pub fn to_local(value: i64, session: &TemporalSession) -> i64 {
    if value == TIMESTAMP_NOBEGIN || value == TIMESTAMP_NOEND {
        return value;
    }
    session.zone.utc_to_local(value)
}

/// The instant of a local wall time in the session zone.
#[must_use]
pub fn from_local(value: i64, session: &TemporalSession) -> i64 {
    if value == TIMESTAMP_NOBEGIN || value == TIMESTAMP_NOEND {
        return value;
    }
    session.zone.local_to_utc(value)
}

/// Converts between temporal carriers as PostgreSQL's casts do.
///
/// # Errors
/// Returns `42846` for unsupported pairs and `22008` for range errors.
pub fn convert(
    value: &TypedValue,
    target: ExtendedType,
    session: &TemporalSession,
) -> Result<TypedValue, TypeInputError> {
    let converted = match (value, target) {
        (TypedValue::Date(date), ExtendedType::Date) => TypedValue::Date(*date),
        (TypedValue::Date(date), ExtendedType::Timestamp { .. }) => {
            TypedValue::Timestamp(date_to_timestamp(*date)?)
        }
        (TypedValue::Date(date), ExtendedType::TimestampTz { .. }) => {
            TypedValue::TimestampTz(from_local(date_to_timestamp(*date)?, session))
        }
        (TypedValue::Timestamp(value), ExtendedType::Date) => {
            TypedValue::Date(timestamp_to_date(*value)?)
        }
        (TypedValue::TimestampTz(value), ExtendedType::Date) => {
            TypedValue::Date(timestamp_to_date(to_local(*value, session))?)
        }
        (TypedValue::Timestamp(value), ExtendedType::Timestamp { .. }) => {
            TypedValue::Timestamp(*value)
        }
        (TypedValue::TimestampTz(value), ExtendedType::TimestampTz { .. }) => {
            TypedValue::TimestampTz(*value)
        }
        (TypedValue::Timestamp(value), ExtendedType::TimestampTz { .. }) => {
            TypedValue::TimestampTz(from_local(*value, session))
        }
        (TypedValue::TimestampTz(value), ExtendedType::Timestamp { .. }) => {
            TypedValue::Timestamp(to_local(*value, session))
        }
        (TypedValue::Timestamp(value), ExtendedType::Time { .. }) if !is_infinite(*value) => {
            TypedValue::Time(value.rem_euclid(USECS_PER_DAY))
        }
        (TypedValue::TimestampTz(value), ExtendedType::Time { .. }) if !is_infinite(*value) => {
            TypedValue::Time(to_local(*value, session).rem_euclid(USECS_PER_DAY))
        }
        (TypedValue::TimestampTz(value), ExtendedType::TimeTz { .. }) if !is_infinite(*value) => {
            let east = session.zone.offset_at(*value);
            let local = value + i64::from(east) * USECS_PER_SEC;
            TypedValue::TimeTz(TimeTz {
                time: local.rem_euclid(USECS_PER_DAY),
                zone: -east,
            })
        }
        (TypedValue::Time(time), ExtendedType::Time { .. }) => TypedValue::Time(*time),
        (TypedValue::TimeTz(value), ExtendedType::TimeTz { .. }) => TypedValue::TimeTz(*value),
        (TypedValue::Time(time), ExtendedType::TimeTz { .. }) => {
            let today = session
                .zone
                .utc_to_local(session.transaction_start)
                .div_euclid(USECS_PER_DAY);
            let east = session.zone.offset_for_local(today * USECS_PER_DAY + time);
            TypedValue::TimeTz(TimeTz {
                time: *time,
                zone: -east,
            })
        }
        (TypedValue::TimeTz(value), ExtendedType::Time { .. }) => TypedValue::Time(value.time),
        (TypedValue::Time(time), ExtendedType::Interval { .. }) => {
            TypedValue::Interval(Interval::new(0, 0, *time))
        }
        (TypedValue::Interval(value), ExtendedType::Interval { .. }) => {
            TypedValue::Interval(*value)
        }
        (TypedValue::Interval(value), ExtendedType::Time { .. }) if !value.is_infinite() => {
            TypedValue::Time(value.micros.rem_euclid(USECS_PER_DAY))
        }
        (value, target) => {
            return Err(cannot_cast(display_name(value.kind()), target));
        }
    };
    apply_typmod(converted, target)
}

fn is_infinite(value: i64) -> bool {
    value == TIMESTAMP_NOBEGIN || value == TIMESTAMP_NOEND
}

/// Applies a target typmod (precision rounding or interval fields).
///
/// # Errors
/// Returns `22008` when interval rounding overflows.
pub fn apply_typmod(value: TypedValue, target: ExtendedType) -> Result<TypedValue, TypeInputError> {
    Ok(match (value, target) {
        (TypedValue::Time(time), ExtendedType::Time { precision }) => {
            TypedValue::Time(round_time(time, precision))
        }
        (TypedValue::TimeTz(value), ExtendedType::TimeTz { precision }) => {
            TypedValue::TimeTz(TimeTz {
                time: round_time(value.time, precision),
                zone: value.zone,
            })
        }
        (TypedValue::Timestamp(value), ExtendedType::Timestamp { precision }) => {
            TypedValue::Timestamp(round_timestamp(value, precision))
        }
        (TypedValue::TimestampTz(value), ExtendedType::TimestampTz { precision }) => {
            TypedValue::TimestampTz(round_timestamp(value, precision))
        }
        (TypedValue::Interval(value), ExtendedType::Interval { typmod }) => TypedValue::Interval(
            value
                .apply_typmod(typmod)
                .map_err(|_| temporal::range_error("interval out of range"))?,
        ),
        (value, _) => value,
    })
}

/// Decodes a text bind parameter of a temporal type. A `timestamptz` or
/// `timetz` without an explicit zone stays text so the executing statement's
/// `TimeZone` interprets it (`None`).
#[must_use]
pub fn parameter_from_text(
    kind: ExtendedType,
    text: &str,
) -> Option<Result<TypedValue, TypeInputError>> {
    let utc = temporal::default_session();
    let parsed = parse_text(kind, text, utc);
    if matches!(
        kind,
        ExtendedType::TimestampTz { .. } | ExtendedType::TimeTz { .. }
    ) {
        let shifted = TemporalSession {
            zone: temporal::zone::fixed_zone(3600),
            ..utc.clone()
        };
        if parsed.as_ref().ok() != parse_text(kind, text, &shifted).as_ref().ok() {
            return None;
        }
    }
    Some(parsed)
}

/// Reads a stored `date`/`time`/`timestamp` column value natively; the
/// fixed-width canonical forms take a fast path.
#[must_use]
pub fn column_value(
    value: &serde_json::Value,
    data_type: &crate::types::DataType,
) -> Option<TypedValue> {
    use crate::types::DataType;
    let text = value.as_str()?;
    match data_type {
        DataType::Date => crate::types::temporal::parse_date(text)
            .ok()
            .map(|date| TypedValue::Date(legacy_days(date)))
            .or_else(|| parse_text(ExtendedType::Date, text, temporal::default_session()).ok()),
        DataType::Time => crate::types::temporal::parse_time(text)
            .ok()
            .map(|time| TypedValue::Time(legacy_micros(time)))
            .or_else(|| {
                parse_text(
                    ExtendedType::Time { precision: None },
                    text,
                    temporal::default_session(),
                )
                .ok()
            }),
        DataType::Timestamp => crate::types::temporal::parse_timestamp(text)
            .ok()
            .map(|value| {
                TypedValue::Timestamp(
                    i64::from(legacy_days(value.date())) * USECS_PER_DAY
                        + legacy_micros(value.time()),
                )
            })
            .or_else(|| {
                parse_text(
                    ExtendedType::Timestamp { precision: None },
                    text,
                    temporal::default_session(),
                )
                .ok()
            }),
        _ => None,
    }
}

/// The fixed-width text the existing `date`/`time`/`timestamp` storage,
/// time-series and retention paths use (`YYYY-MM-DDTHH:MM:SS.ffffffZ`).
#[must_use]
pub fn legacy_text(value: &TypedValue) -> Option<String> {
    let text = value.text();
    match value {
        TypedValue::Date(_) => crate::types::temporal::canonical_date(&text).ok(),
        TypedValue::Time(_) => crate::types::temporal::canonical_time(&text).ok(),
        TypedValue::Timestamp(_) => crate::types::temporal::canonical_timestamp(&text).ok(),
        TypedValue::TimestampTz(instant) => {
            let utc = TemporalSession {
                zone: temporal::zone::Zone::utc(),
                ..temporal::default_session().clone()
            };
            let local = TypedValue::Timestamp(to_local(*instant, &utc));
            crate::types::temporal::canonical_timestamp(&local.text()).ok()
        }
        _ => None,
    }
}

fn legacy_days(date: time::Date) -> i32 {
    date.to_julian_day() - POSTGRES_EPOCH_JDATE
}

fn legacy_micros(time: time::Time) -> i64 {
    super::temporal::calendar::time_value(
        i32::from(time.hour()),
        i32::from(time.minute()),
        i32::from(time.second()),
        i64::from(time.microsecond()),
    )
}

pub(super) fn receive_binary(kind: ExtendedType, bytes: &[u8]) -> Result<TypedValue, String> {
    let i64_frame =
        |bytes: &[u8], name: &str| temporal::fixed::<8>(bytes, name).map(i64::from_be_bytes);
    Ok(match kind {
        ExtendedType::Date => {
            TypedValue::Date(i32::from_be_bytes(temporal::fixed::<4>(bytes, "date")?))
        }
        ExtendedType::Time { .. } => TypedValue::Time(i64_frame(bytes, "time")?),
        ExtendedType::TimeTz { .. } => TypedValue::TimeTz(temporal::receive_timetz(bytes)?),
        ExtendedType::Timestamp { .. } => TypedValue::Timestamp(i64_frame(bytes, "timestamp")?),
        ExtendedType::TimestampTz { .. } => {
            TypedValue::TimestampTz(i64_frame(bytes, "timestamptz")?)
        }
        ExtendedType::Interval { .. } => TypedValue::Interval(temporal::receive_interval(bytes)?),
        _ => return Err(format!("invalid binary {} frame", kind.name())),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_type_name;
    use crate::types::extended::ExtendedType;
    use crate::types::DataType;

    #[test]
    fn should_parse_temporal_type_spellings_with_typmods() {
        // Arrange
        let names = [
            "timestamp(3) with time zone",
            "timestamptz",
            "time(2)",
            "interval second(3)",
            "interval",
        ];

        // Act
        let parsed = names.map(|name| parse_type_name(name).and_then(Result::ok));

        // Assert
        assert_eq!(
            parsed,
            [
                Some(DataType::Extended(ExtendedType::TimestampTz {
                    precision: Some(3)
                })),
                Some(DataType::Extended(ExtendedType::TimestampTz {
                    precision: None
                })),
                Some(DataType::Extended(ExtendedType::Time {
                    precision: Some(2)
                })),
                Some(DataType::Extended(ExtendedType::Interval {
                    typmod: 268_435_459
                })),
                Some(DataType::Extended(ExtendedType::Interval { typmod: -1 })),
            ]
        );
        assert_eq!(parse_type_name("timestamp"), None);
    }
}
