//! Shared temporal parsing and canonicalization.
//!
//! This is the one parser used for DATE/TIME/TIMESTAMP values on every path
//! that accepts them as strings: row storage, casts, the pgwire binary
//! codec, `time_bucket`, and retention. Timestamps with an explicit offset
//! are converted to UTC; offset-free timestamps are stored as given. Storing
//! the canonical, zero-padded form means plain byte/string comparison
//! (`ORDER BY`, equality) agrees with chronological order.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

/// Parses and re-serializes a DATE string into its canonical `YYYY-MM-DD` form.
///
/// # Errors
///
/// Returns an error when `value` is not a valid `YYYY-MM-DD` date.
pub fn canonical_date(value: &str) -> Result<String, String> {
    parse_date(value).map(format_date)
}

/// Parses and re-serializes a TIME string into its canonical
/// `HH:MM:SS[.ffffff]` form.
///
/// # Errors
///
/// Returns an error when `value` is not a valid `HH:MM:SS[.ffffff]` time.
pub fn canonical_time(value: &str) -> Result<String, String> {
    parse_time(value).map(format_time)
}

/// Parses a TIMESTAMP string, converting any explicit offset to UTC, and
/// re-serializes it into the canonical `YYYY-MM-DDTHH:MM:SS[.ffffff]Z` form.
/// The trailing `Z` is always present (even for offset-free input, which is
/// treated as already UTC) so every stored timestamp is both directly
/// RFC3339-parseable and sorts correctly as plain text.
///
/// # Errors
///
/// Returns an error when `value` is not a valid RFC3339 or
/// `YYYY-MM-DD[T ]HH:MM:SS[.ffffff]` timestamp.
pub fn canonical_timestamp(value: &str) -> Result<String, String> {
    let datetime = parse_timestamp(value)?;
    Ok(format!(
        "{}T{}Z",
        format_date(datetime.date()),
        format_time(datetime.time())
    ))
}

/// # Errors
///
/// Returns an error when `value` is not a valid `YYYY-MM-DD` date.
pub fn parse_date(value: &str) -> Result<Date, String> {
    let mut parts = value.split('-');
    let year = parts
        .next()
        .ok_or_else(|| invalid("date"))?
        .parse::<i32>()
        .map_err(|_| invalid("date"))?;
    let month = parts
        .next()
        .ok_or_else(|| invalid("date"))?
        .parse::<u8>()
        .map_err(|_| invalid("date"))?;
    let day = parts
        .next()
        .ok_or_else(|| invalid("date"))?
        .parse::<u8>()
        .map_err(|_| invalid("date"))?;
    if parts.next().is_some() {
        return Err(invalid("date"));
    }
    let month = Month::try_from(month).map_err(|_| invalid("date"))?;
    Date::from_calendar_date(year, month, day).map_err(|_| invalid("date"))
}

/// # Errors
///
/// Returns an error when `value` is not a valid `HH:MM:SS[.ffffff]` time.
pub fn parse_time(value: &str) -> Result<Time, String> {
    let (clock, fraction) = value.split_once('.').unwrap_or((value, ""));
    let mut parts = clock.split(':');
    let hour = parts
        .next()
        .ok_or_else(|| invalid("time"))?
        .parse::<u8>()
        .map_err(|_| invalid("time"))?;
    let minute = parts
        .next()
        .ok_or_else(|| invalid("time"))?
        .parse::<u8>()
        .map_err(|_| invalid("time"))?;
    let second = parts
        .next()
        .ok_or_else(|| invalid("time"))?
        .parse::<u8>()
        .map_err(|_| invalid("time"))?;
    if parts.next().is_some()
        || fraction.len() > 6
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid("time"));
    }
    let micros = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u32>()
            .map_err(|_| invalid("time"))?
            .saturating_mul(10_u32.pow(6 - u32::try_from(fraction.len()).unwrap_or(6)))
    };
    Time::from_hms_micro(hour, minute, second, micros).map_err(|_| invalid("time"))
}

/// # Errors
///
/// Returns an error when `value` is not a valid RFC3339 or
/// `YYYY-MM-DD[T ]HH:MM:SS[.ffffff]` timestamp.
pub fn parse_timestamp(value: &str) -> Result<PrimitiveDateTime, String> {
    if let Ok(datetime) =
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
    {
        let datetime = datetime.to_offset(UtcOffset::UTC);
        return Ok(PrimitiveDateTime::new(datetime.date(), datetime.time()));
    }

    let normalized = value.replace(' ', "T");
    let (date, time) = normalized
        .split_once('T')
        .ok_or_else(|| invalid("timestamp"))?;
    Ok(PrimitiveDateTime::new(parse_date(date)?, parse_time(time)?))
}

#[must_use]
pub fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

#[must_use]
pub fn format_time(time: Time) -> String {
    if time.microsecond() == 0 {
        format!(
            "{:02}:{:02}:{:02}",
            time.hour(),
            time.minute(),
            time.second()
        )
    } else {
        format!(
            "{:02}:{:02}:{:02}.{:06}",
            time.hour(),
            time.minute(),
            time.second(),
            time.microsecond()
        )
    }
}

fn invalid(kind: &str) -> String {
    format!("invalid {kind} value")
}

#[cfg(test)]
mod tests {
    use super::{canonical_date, canonical_time, canonical_timestamp};

    #[test]
    fn should_reject_an_invalid_timestamp_string() {
        // Arrange
        let value = "not-a-timestamp";

        // Act
        let result = canonical_timestamp(value);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_convert_an_offset_timestamp_to_canonical_utc() {
        // Arrange
        let value = "2024-01-01T09:00:00+02:00";

        // Act
        let canonical = canonical_timestamp(value).expect("valid timestamp");

        // Assert
        assert_eq!(canonical, "2024-01-01T07:00:00Z");
    }

    #[test]
    fn should_order_mixed_offset_timestamps_by_instant_after_canonicalization() {
        // Arrange
        let earlier = canonical_timestamp("2024-01-01T09:00:00+02:00").expect("earlier");
        let later = canonical_timestamp("2024-01-01T08:00:00Z").expect("later");

        // Act & Assert: 07:00Z < 08:00Z, so plain string comparison must agree.
        assert!(earlier < later);
    }

    #[test]
    fn should_reject_an_invalid_date_string() {
        assert!(canonical_date("2024-13-40").is_err());
    }

    #[test]
    fn should_reject_an_invalid_time_string() {
        assert!(canonical_time("25:00:00").is_err());
    }
}
