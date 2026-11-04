//! `EncodeDateOnly` / `EncodeTimeOnly` / `EncodeDateTime`: text output in
//! every `DateStyle`.
use super::calendar::{
    date2j, date_to_fields, j2day, time_fields, timestamp_to_fields, Fields, DATE_NOBEGIN,
    DATE_NOEND, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND, USECS_PER_SEC,
};
use super::interval::append_seconds;
use super::session::{DateOrder, DateOutput, TemporalSession};
use std::fmt::Write as _;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn era_year(year: i32) -> i32 {
    if year > 0 {
        year
    } else {
        -(year - 1)
    }
}

fn index(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// `EncodeTimezone`: `+hh`, `+hh:mm` or `+hh:mm:ss` from seconds east.
#[must_use]
pub fn encode_offset(east: i32) -> String {
    super::zone::offset_abbreviation(east)
}

/// `date_out`.
#[must_use]
pub fn format_date(date: i32, session: &TemporalSession) -> String {
    match date {
        DATE_NOBEGIN => return "-infinity".to_string(),
        DATE_NOEND => return "infinity".to_string(),
        _ => {}
    }
    let (year, month, day) = date_to_fields(date);
    let shown = era_year(year);
    let mut out = match (session.date_output, session.date_order) {
        (DateOutput::Iso, _) => format!("{shown:04}-{month:02}-{day:02}"),
        (DateOutput::Sql, DateOrder::Dmy) => format!("{day:02}/{month:02}/{shown:04}"),
        (DateOutput::Sql, _) => format!("{month:02}/{day:02}/{shown:04}"),
        (DateOutput::German, _) => format!("{day:02}.{month:02}.{shown:04}"),
        (DateOutput::Postgres, DateOrder::Dmy) => format!("{day:02}-{month:02}-{shown:04}"),
        (DateOutput::Postgres, _) => format!("{month:02}-{day:02}-{shown:04}"),
    };
    if year <= 0 {
        out.push_str(" BC");
    }
    out
}

fn clock(out: &mut String, hour: i32, minute: i32, second: i32, micros: i64) {
    let _ = write!(out, "{hour:02}:{minute:02}:");
    append_seconds(out, i64::from(second), micros, true);
}

/// `time_out`.
#[must_use]
pub fn format_time(time: i64) -> String {
    let (hour, minute, second, micros) = time_fields(time);
    let mut out = String::new();
    clock(&mut out, hour, minute, second, micros);
    out
}

/// `timetz_out`; `zone_west` is PostgreSQL's seconds-west displacement.
#[must_use]
pub fn format_timetz(time: i64, zone_west: i32) -> String {
    let mut out = format_time(time);
    out.push_str(&encode_offset(-zone_west));
    out
}

/// `timestamp_out`.
#[must_use]
pub fn format_timestamp(value: i64, session: &TemporalSession) -> String {
    match value {
        TIMESTAMP_NOBEGIN => return "-infinity".to_string(),
        TIMESTAMP_NOEND => return "infinity".to_string(),
        _ => {}
    }
    timestamp_to_fields(value).map_or_else(
        || "timestamp out of range".to_string(),
        |fields| encode_datetime(&fields, None, session, false),
    )
}

/// `timestamptz_out` in the session time zone.
#[must_use]
pub fn format_timestamptz(value: i64, session: &TemporalSession) -> String {
    match value {
        TIMESTAMP_NOBEGIN => return "-infinity".to_string(),
        TIMESTAMP_NOEND => return "infinity".to_string(),
        _ => {}
    }
    let info = session.zone.info_at(value);
    let local = value.saturating_add(i64::from(info.east) * USECS_PER_SEC);
    timestamp_to_fields(local).map_or_else(
        || "timestamp out of range".to_string(),
        |fields| {
            encode_datetime(
                &fields,
                Some((info.east, &info.abbreviation)),
                session,
                false,
            )
        },
    )
}

/// JSON timestamps use ISO calendar spelling and explicit timezone minutes.
/// SQL TIMETZ output deliberately retains its established short-offset rule.
#[must_use]
pub(crate) fn format_json_timestamp(
    value: i64,
    session: &TemporalSession,
    with_timezone: bool,
) -> String {
    match value {
        TIMESTAMP_NOBEGIN => return "-infinity".to_owned(),
        TIMESTAMP_NOEND => return "infinity".to_owned(),
        _ => {}
    }
    let east = with_timezone.then(|| session.zone.offset_at(value));
    let local = east.map_or(value, |east| {
        value.saturating_add(i64::from(east) * USECS_PER_SEC)
    });
    let mut iso = session.clone();
    iso.date_output = DateOutput::Iso;
    timestamp_to_fields(local).map_or_else(
        || "timestamp out of range".to_owned(),
        |fields| encode_datetime(&fields, east.map(|east| (east, "")), &iso, true),
    )
}

fn json_offset(east: i32) -> String {
    let mut offset = encode_offset(east);
    if east % 3600 == 0 {
        offset.push_str(":00");
    }
    offset
}

fn encode_datetime(
    fields: &Fields,
    zone: Option<(i32, &str)>,
    session: &TemporalSession,
    json: bool,
) -> String {
    let year = era_year(fields.year);
    let (month, day) = (fields.month, fields.day);
    let mut out = String::new();
    let abbreviation = |out: &mut String, numeric_space: bool| {
        if let Some((east, name)) = zone {
            if name.is_empty() {
                if numeric_space {
                    out.push(' ');
                }
                out.push_str(&encode_offset(east));
            } else {
                out.push(' ');
                out.push_str(name);
            }
        }
    };
    match session.date_output {
        DateOutput::Iso => {
            let _ = write!(out, "{year:04}-{month:02}-{day:02}");
            out.push(if json { 'T' } else { ' ' });
            clock(
                &mut out,
                fields.hour,
                fields.minute,
                fields.second,
                fields.micros,
            );
            if let Some((east, _)) = zone {
                out.push_str(&if json {
                    json_offset(east)
                } else {
                    encode_offset(east)
                });
            }
        }
        DateOutput::Sql => {
            if session.date_order == DateOrder::Dmy {
                let _ = write!(out, "{day:02}/{month:02}");
            } else {
                let _ = write!(out, "{month:02}/{day:02}");
            }
            let _ = write!(out, "/{year:04} ");
            clock(
                &mut out,
                fields.hour,
                fields.minute,
                fields.second,
                fields.micros,
            );
            abbreviation(&mut out, false);
        }
        DateOutput::German => {
            let _ = write!(out, "{day:02}.{month:02}.{year:04} ");
            clock(
                &mut out,
                fields.hour,
                fields.minute,
                fields.second,
                fields.micros,
            );
            abbreviation(&mut out, false);
        }
        DateOutput::Postgres => {
            let weekday = j2day(date2j(fields.year, month, day));
            out.push_str(DAYS[index(weekday)]);
            out.push(' ');
            let month_name = MONTHS[index(month - 1)];
            if session.date_order == DateOrder::Dmy {
                let _ = write!(out, "{day:02} {month_name}");
            } else {
                let _ = write!(out, "{month_name} {day:02}");
            }
            out.push(' ');
            clock(
                &mut out,
                fields.hour,
                fields.minute,
                fields.second,
                fields.micros,
            );
            let _ = write!(out, " {year:04}");
            abbreviation(&mut out, true);
        }
    }
    if fields.year <= 0 {
        out.push_str(" BC");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_date, format_time, format_timestamptz, format_timetz};
    use crate::types::extended::temporal::session::{DateOrder, DateOutput, TemporalSession};
    use crate::types::extended::temporal::zone::lookup_zone;

    #[test]
    fn should_keep_json_timestamp_offset_minutes_separate_from_sql_output() {
        // Arrange
        let session = super::TemporalSession::default();
        // Act
        let sql = super::format_timestamptz(0, &session);
        let json = super::format_json_timestamp(0, &session, true);
        let timestamp = super::format_json_timestamp(0, &session, false);
        // Assert
        assert_eq!(sql, "2000-01-01 00:00:00+00");
        assert_eq!(json, "2000-01-01T00:00:00+00:00");
        assert_eq!(timestamp, "2000-01-01T00:00:00");
    }

    #[test]
    fn should_format_iso_temporal_output() {
        // Arrange
        let session = TemporalSession::default();

        // Act
        let date = format_date(1, &session);
        let bc = format_date(-730_485, &session);
        let time = format_time(11_045_006_007);
        let timetz = format_timetz(11_045_006_007, -7200);

        // Assert
        assert_eq!(date, "2000-01-02");
        assert_eq!(bc, "0001-01-01 BC");
        assert_eq!(time, "03:04:05.006007");
        assert_eq!(timetz, "03:04:05.006007+02");
    }

    #[test]
    fn should_format_timestamptz_using_session_settings() {
        // Arrange
        let zone = lookup_zone("America/New_York").expect("zone");
        let iso = TemporalSession {
            zone: zone.clone(),
            ..TemporalSession::default()
        };
        let postgres = TemporalSession {
            zone,
            date_output: DateOutput::Postgres,
            date_order: DateOrder::Mdy,
            ..TemporalSession::default()
        };

        // Act
        let iso_text = format_timestamptz(0, &iso);
        let postgres_text = format_timestamptz(0, &postgres);

        // Assert
        assert_eq!(iso_text, "1999-12-31 19:00:00-05");
        assert_eq!(postgres_text, "Fri Dec 31 19:00:00 1999 EST");
    }
}
