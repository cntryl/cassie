//! Date/time families: `date`, `time`, `timetz`, `timestamp`, `timestamptz`
//! and `interval`, ported from PostgreSQL's `datetime.c`, `date.c`,
//! `timestamp.c` and the bundled IANA tz database.
use super::TypeInputError;

#[path = "temporal/calendar.rs"]
pub mod calendar;
#[path = "temporal/decode.rs"]
pub mod decode;
#[path = "temporal/encode.rs"]
pub mod encode;
#[path = "temporal/format.rs"]
pub mod format;
#[path = "temporal/functions.rs"]
pub mod functions;
#[path = "temporal/interval.rs"]
pub mod interval;
#[path = "temporal/ops.rs"]
pub mod ops;
#[path = "temporal/session.rs"]
pub mod session;
#[path = "temporal/tokens.rs"]
pub mod tokens;
#[path = "temporal/zone.rs"]
pub mod zone;

use calendar::{
    date2j, fields_to_timestamp, is_valid_date, is_valid_julian, round_time, round_timestamp,
    DATE_NOBEGIN, DATE_NOEND, POSTGRES_EPOCH_JDATE, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND,
    UNIX_TO_POSTGRES_SECS, USECS_PER_SEC,
};
use decode::{decode_datetime, decode_time_only, Special};
pub use interval::Interval;
pub use session::TemporalSession;
use tokens::DecodeError;

/// `timetz`: time of day and PostgreSQL's seconds-west zone displacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TimeTz {
    pub time: i64,
    pub zone: i32,
}

impl TimeTz {
    /// `timetz_cmp` order: the UTC-equivalent time, then the zone.
    #[must_use]
    pub fn order_key(&self) -> (i64, i32) {
        (self.time + i64::from(self.zone) * USECS_PER_SEC, self.zone)
    }
}

/// The shared default session used where no statement session is known.
#[must_use]
pub fn default_session() -> &'static TemporalSession {
    static DEFAULT: std::sync::LazyLock<TemporalSession> =
        std::sync::LazyLock::new(TemporalSession::default);
    &DEFAULT
}

/// Maps a decoder error to PostgreSQL's message and SQLSTATE.
#[must_use]
pub fn input_error(type_name: &str, input: &str, error: &DecodeError) -> TypeInputError {
    let (sqlstate, message) = match error {
        DecodeError::BadFormat => (
            "22007",
            format!("invalid input syntax for type {type_name}: \"{input}\""),
        ),
        DecodeError::FieldOverflow | DecodeError::MdFieldOverflow => (
            "22008",
            format!("date/time field value out of range: \"{input}\""),
        ),
        DecodeError::IntervalOverflow => (
            "22015",
            format!("interval field value out of range: \"{input}\""),
        ),
        DecodeError::TzDispOverflow => (
            "22009",
            format!("time zone displacement out of range: \"{input}\""),
        ),
        DecodeError::BadTimezone(zone) => ("22023", format!("time zone \"{zone}\" not recognized")),
    };
    TypeInputError { sqlstate, message }
}

/// A `22008` range error such as `timestamp out of range`.
#[must_use]
pub fn range_error(message: impl Into<String>) -> TypeInputError {
    TypeInputError {
        sqlstate: "22008",
        message: message.into(),
    }
}

const EPOCH_TIMESTAMP: i64 = -UNIX_TO_POSTGRES_SECS * USECS_PER_SEC;

/// `date_in`.
///
/// # Errors
/// Returns PostgreSQL's input error.
pub fn parse_date(text: &str, session: &TemporalSession) -> Result<i32, TypeInputError> {
    let decoded =
        decode_datetime(text, session).map_err(|error| input_error("date", text, &error))?;
    match decoded.special {
        Some(Special::Late) => return Ok(DATE_NOEND),
        Some(Special::Early) => return Ok(DATE_NOBEGIN),
        Some(Special::Epoch) => return Ok(-10_957),
        None => {}
    }
    let fields = decoded.fields;
    let out_of_range = || range_error(format!("date out of range: \"{text}\""));
    if !is_valid_julian(fields.year, fields.month, fields.day) {
        return Err(out_of_range());
    }
    let date = date2j(fields.year, fields.month, fields.day) - i64::from(POSTGRES_EPOCH_JDATE);
    if !is_valid_date(date) {
        return Err(out_of_range());
    }
    i32::try_from(date).map_err(|_| out_of_range())
}

/// `timestamp_in`; offsets in the text are ignored.
///
/// # Errors
/// Returns PostgreSQL's input error.
pub fn parse_timestamp(
    text: &str,
    session: &TemporalSession,
    precision: Option<u8>,
) -> Result<i64, TypeInputError> {
    let decoded =
        decode_datetime(text, session).map_err(|error| input_error("timestamp", text, &error))?;
    let value = match decoded.special {
        Some(Special::Late) => TIMESTAMP_NOEND,
        Some(Special::Early) => TIMESTAMP_NOBEGIN,
        Some(Special::Epoch) => EPOCH_TIMESTAMP,
        None => fields_to_timestamp(&decoded.fields, None)
            .ok_or_else(|| range_error(format!("timestamp out of range: \"{text}\"")))?,
    };
    Ok(round_timestamp(value, precision))
}

/// `timestamptz_in`: offset-free text is local time in the session zone.
///
/// # Errors
/// Returns PostgreSQL's input error.
pub fn parse_timestamptz(
    text: &str,
    session: &TemporalSession,
    precision: Option<u8>,
) -> Result<i64, TypeInputError> {
    let decoded = decode_datetime(text, session)
        .map_err(|error| input_error("timestamp with time zone", text, &error))?;
    let value = match decoded.special {
        Some(Special::Late) => TIMESTAMP_NOEND,
        Some(Special::Early) => TIMESTAMP_NOBEGIN,
        Some(Special::Epoch) => EPOCH_TIMESTAMP,
        None => fields_to_timestamp(&decoded.fields, Some(decoded.zone_west))
            .ok_or_else(|| range_error(format!("timestamp out of range: \"{text}\"")))?,
    };
    Ok(round_timestamp(value, precision))
}

/// `time_in`.
///
/// # Errors
/// Returns PostgreSQL's input error.
pub fn parse_time(
    text: &str,
    session: &TemporalSession,
    precision: Option<u8>,
) -> Result<i64, TypeInputError> {
    let decoded =
        decode_time_only(text, session).map_err(|error| input_error("time", text, &error))?;
    Ok(round_time(decoded.time, precision))
}

/// `timetz_in`.
///
/// # Errors
/// Returns PostgreSQL's input error.
pub fn parse_timetz(
    text: &str,
    session: &TemporalSession,
    precision: Option<u8>,
) -> Result<TimeTz, TypeInputError> {
    let decoded = decode_time_only(text, session)
        .map_err(|error| input_error("time with time zone", text, &error))?;
    Ok(TimeTz {
        time: round_time(decoded.time, precision),
        zone: decoded.zone_west,
    })
}

/// `interval_in` with a PostgreSQL interval typmod (`-1` for none).
///
/// # Errors
/// Returns PostgreSQL's input error.
pub fn parse_interval(
    text: &str,
    typmod: i32,
    session: &TemporalSession,
) -> Result<Interval, TypeInputError> {
    let range = if typmod < 0 {
        interval::FULL_RANGE
    } else {
        (typmod >> 16) & 0x7FFF
    };
    let value = interval::parse_interval(text, range, session.interval_style).map_err(|error| {
        let error = if error == DecodeError::FieldOverflow {
            DecodeError::IntervalOverflow
        } else {
            error
        };
        input_error("interval", text, &error)
    })?;
    value
        .apply_typmod(typmod)
        .map_err(|_| range_error("interval out of range"))
}

/// Reads exactly `N` big-endian bytes.
///
/// # Errors
/// Returns a message for a frame of the wrong length.
pub fn fixed<const N: usize>(bytes: &[u8], name: &str) -> Result<[u8; N], String> {
    <[u8; N]>::try_from(bytes).map_err(|_| format!("invalid binary {name} frame"))
}

/// `interval_recv` frame: time (i64), day (i32), month (i32).
///
/// # Errors
/// Returns a message for a frame of the wrong length.
pub fn receive_interval(bytes: &[u8]) -> Result<Interval, String> {
    let frame = fixed::<16>(bytes, "interval")?;
    let micros = i64::from_be_bytes(frame[..8].try_into().map_err(|_| "interval")?);
    let days = i32::from_be_bytes(frame[8..12].try_into().map_err(|_| "interval")?);
    let months = i32::from_be_bytes(frame[12..].try_into().map_err(|_| "interval")?);
    Ok(Interval::new(months, days, micros))
}

#[must_use]
pub fn send_interval(value: &Interval) -> Vec<u8> {
    let mut out = value.micros.to_be_bytes().to_vec();
    out.extend_from_slice(&value.days.to_be_bytes());
    out.extend_from_slice(&value.months.to_be_bytes());
    out
}

/// `timetz_recv` frame: time (i64), zone seconds west (i32).
///
/// # Errors
/// Returns a message for a frame of the wrong length.
pub fn receive_timetz(bytes: &[u8]) -> Result<TimeTz, String> {
    let frame = fixed::<12>(bytes, "timetz")?;
    let time = i64::from_be_bytes(frame[..8].try_into().map_err(|_| "timetz")?);
    let zone = i32::from_be_bytes(frame[8..].try_into().map_err(|_| "timetz")?);
    Ok(TimeTz { time, zone })
}

#[must_use]
pub fn send_timetz(value: &TimeTz) -> Vec<u8> {
    let mut out = value.time.to_be_bytes().to_vec();
    out.extend_from_slice(&value.zone.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::{parse_date, parse_interval, parse_timestamp, parse_timestamptz, TemporalSession};

    #[test]
    fn should_parse_temporal_text_with_postgresql_sqlstates() {
        // Arrange
        let session = TemporalSession::default();

        // Act
        let date = parse_date("2000-01-02", &session);
        let infinite = parse_date("infinity", &session);
        let bad = parse_date("garbage", &session).map_err(|error| error.sqlstate);
        let overflow =
            parse_timestamp("2001-13-01", &session, None).map_err(|error| error.sqlstate);
        let interval =
            parse_interval("1 mon 2 days 03:04:05.006007", -1, &session).map(|value| value.micros);
        let rounded = parse_timestamp("2001-02-16 20:38:40.1235", &session, Some(3))
            == parse_timestamp("2001-02-16 20:38:40.124", &session, None);
        let shifted = parse_timestamptz("2023-12-31 19:00:00-05", &session, None)
            == parse_timestamptz("2024-01-01 00:00:00+00", &session, None);

        // Assert
        assert_eq!(date.ok(), Some(1));
        assert_eq!(infinite.ok(), Some(i32::MAX));
        assert_eq!(bad, Err("22007"));
        assert_eq!(overflow, Err("22008"));
        assert_eq!(interval.ok(), Some(11_045_006_007));
        assert!(rounded && shifted);
    }
}
