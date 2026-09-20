//! Shared temporal parsing and canonicalization.
//!
//! This is the one parser used for DATE/TIME/TIMESTAMP values on every path
//! that accepts them as strings: row storage, casts, the pgwire binary
//! codec, `time_bucket`, time-series indexes, and retention. Timestamps with
//! an explicit offset are converted to UTC; offset-free timestamps are
//! stored as given. The canonical TIMESTAMP form is fixed width
//! (`YYYY-MM-DDTHH:MM:SS.ffffffZ`), so plain byte/string comparison agrees
//! with chronological order. Values written before the fixed-width form
//! (`...SSZ` for whole seconds) are ordered through
//! [`timestamp_order_text`], which every executor comparison applies.

use std::borrow::Cow;

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
/// re-serializes it into the canonical fixed-width
/// `YYYY-MM-DDTHH:MM:SS.ffffffZ` form. The fraction and trailing `Z` are
/// always present (offset-free input is treated as already UTC) so every
/// stored timestamp is directly RFC3339-parseable and sorts chronologically
/// as plain text: a variable-width form would put `12:00:00.5Z` before
/// `12:00:00Z` because `.` sorts before `Z`.
///
/// # Errors
///
/// Returns an error when `value` is not a valid RFC3339 or
/// `YYYY-MM-DD[T ]HH:MM:SS[.ffffff]` timestamp.
pub fn canonical_timestamp(value: &str) -> Result<String, String> {
    let datetime = parse_timestamp(value)?;
    let time = datetime.time();
    Ok(format!(
        "{}T{:02}:{:02}:{:02}.{:06}Z",
        format_date(datetime.date()),
        time.hour(),
        time.minute(),
        time.second(),
        time.microsecond()
    ))
}

/// Returns the text that orders `value` chronologically against other
/// canonical timestamps. A string shaped like a canonical UTC timestamp
/// (`YYYY-MM-DDTHH:MM:SS[.f{1,6}]Z`, including the variable-width form
/// written before timestamps became fixed width) is widened to the
/// fixed-width `...SS.ffffffZ` form; any other string is returned unchanged.
/// This is a pure text normalization, so it keeps comparisons total and
/// cheap: already fixed-width values are borrowed without allocating.
#[must_use]
pub fn timestamp_order_text(value: &str) -> Cow<'_, str> {
    const FIXED_WIDTH_LEN: usize = 27;
    let Some(fraction) = canonical_timestamp_fraction(value) else {
        return Cow::Borrowed(value);
    };
    if value.len() == FIXED_WIDTH_LEN {
        return Cow::Borrowed(value);
    }
    let mut widened = String::with_capacity(FIXED_WIDTH_LEN);
    widened.push_str(&value[..19]);
    widened.push('.');
    widened.push_str(fraction);
    for _ in fraction.len()..6 {
        widened.push('0');
    }
    widened.push('Z');
    Cow::Owned(widened)
}

/// Returns true when `value` has the canonical UTC timestamp shape
/// `YYYY-MM-DDTHH:MM:SS[.f{1,6}]Z` that [`timestamp_order_text`] widens.
/// Byte-equality shortcuts must not be applied to such values, because the
/// legacy `...SSZ` form and the fixed-width form name the same instant.
#[must_use]
pub fn is_canonical_timestamp_text(value: &str) -> bool {
    canonical_timestamp_fraction(value).is_some()
}

/// Returns the fractional-second digits (possibly empty) when `value` has the
/// canonical `YYYY-MM-DDTHH:MM:SS[.f{1,6}]Z` shape.
fn canonical_timestamp_fraction(value: &str) -> Option<&str> {
    const SHAPE: &[u8; 19] = b"0000-00-00T00:00:00";
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.last() != Some(&b'Z') {
        return None;
    }
    let shape_matches = SHAPE.iter().zip(bytes).all(|(expected, actual)| {
        if *expected == b'0' {
            actual.is_ascii_digit()
        } else {
            actual == expected
        }
    });
    if !shape_matches {
        return None;
    }
    let suffix = &value[19..value.len() - 1];
    if suffix.is_empty() {
        return Some(suffix);
    }
    let digits = suffix.strip_prefix('.')?;
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(digits)
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

/// Parses a TIMESTAMP string with [`parse_timestamp`] and returns it as a UTC
/// instant: explicit offsets are converted and offset-free input is treated
/// as UTC, exactly as stored values are canonicalized.
///
/// # Errors
///
/// Returns an error when `value` is not a valid RFC3339 or
/// `YYYY-MM-DD[T ]HH:MM:SS[.ffffff]` timestamp.
pub fn parse_timestamp_utc(value: &str) -> Result<OffsetDateTime, String> {
    parse_timestamp(value).map(PrimitiveDateTime::assume_utc)
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
    use std::borrow::Cow;

    use super::{
        canonical_date, canonical_time, canonical_timestamp, parse_timestamp_utc,
        timestamp_order_text,
    };

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
        assert_eq!(canonical, "2024-01-01T07:00:00.000000Z");
    }

    #[test]
    fn should_order_mixed_offset_timestamps_by_instant_after_canonicalization() {
        // Arrange
        let earlier = canonical_timestamp("2024-01-01T09:00:00+02:00").expect("earlier");
        let later = canonical_timestamp("2024-01-01T08:00:00Z").expect("later");

        // Act
        let is_earlier_first = earlier < later;

        // Assert: 07:00Z < 08:00Z, so plain string comparison must agree.
        assert!(is_earlier_first);
    }

    #[test]
    fn should_reject_an_invalid_date_string() {
        // Arrange
        let value = "2024-13-40";

        // Act
        let result = canonical_date(value);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_reject_an_invalid_time_string() {
        // Arrange
        let value = "25:00:00";

        // Act
        let result = canonical_time(value);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_emit_fixed_width_canonical_timestamps_that_sort_chronologically() {
        // Arrange
        let whole = "2024-01-01T12:00:00Z";
        let fractional = "2024-01-01T12:00:00.5Z";

        // Act
        let whole = canonical_timestamp(whole).expect("whole second");
        let fractional = canonical_timestamp(fractional).expect("fractional second");

        // Assert
        assert_eq!(whole, "2024-01-01T12:00:00.000000Z");
        assert_eq!(fractional, "2024-01-01T12:00:00.500000Z");
        assert!(whole < fractional);
    }

    #[test]
    fn should_widen_variable_width_canonical_timestamps_for_ordering() {
        // Arrange
        let legacy_whole = "2024-01-01T12:00:00Z";
        let short_fraction = "2024-01-01T12:00:00.25Z";
        let fixed_width = "2024-01-01T12:00:00.500000Z";

        // Act
        let legacy_whole = timestamp_order_text(legacy_whole);
        let short_fraction = timestamp_order_text(short_fraction);
        let fixed_width = timestamp_order_text(fixed_width);

        // Assert
        assert_eq!(legacy_whole, "2024-01-01T12:00:00.000000Z");
        assert_eq!(short_fraction, "2024-01-01T12:00:00.250000Z");
        assert!(matches!(fixed_width, Cow::Borrowed(_)));
        assert!(legacy_whole < short_fraction && short_fraction < fixed_width);
    }

    #[test]
    fn should_parse_offset_and_offset_free_timestamps_to_the_same_utc_instant() {
        // Arrange
        let offset = "2024-01-01T09:00:00.5+02:00";
        let offset_free = "2024-01-01 07:00:00.5";
        let fixed_width = "2024-01-01T07:00:00.500000Z";

        // Act
        let offset = parse_timestamp_utc(offset).expect("offset timestamp");
        let offset_free = parse_timestamp_utc(offset_free).expect("offset-free timestamp");
        let fixed_width = parse_timestamp_utc(fixed_width).expect("fixed-width timestamp");

        // Assert
        assert_eq!(offset, offset_free);
        assert_eq!(offset, fixed_width);
    }

    #[test]
    fn should_leave_non_timestamp_text_unchanged_for_ordering() {
        // Arrange
        let values = [
            "2024-01-01 12:00:00",
            "2024-01-01T12:00:00+02:00",
            "2024-01-01T12:00:00.1234567Z",
            "2024-01-01T12:00:00.Z",
            "2024-01-01",
            "hello",
            "",
        ];

        for value in values {
            // Act
            let ordered = timestamp_order_text(value);

            // Assert
            assert!(
                matches!(ordered, Cow::Borrowed(text) if text == value),
                "{value}"
            );
        }
    }
}
