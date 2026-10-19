//! Validation of the timeout, encoding and `pg_dump` preamble settings,
//! canonicalized as PostgreSQL 18 shows them.
//!
//! `statement_timeout` is enforced as a statement deadline. The remaining
//! settings in this module are accepted with their PostgreSQL values but do
//! not change execution: Cassie has no lock waits bounded by `lock_timeout`,
//! no idle-session reaper, no XML type, no row-level security policies and a
//! single table access method.
use crate::app::CassieError;

/// The largest millisecond value PostgreSQL accepts for a timeout setting.
const MAX_TIMEOUT_MS: i64 = 2_147_483_647;

/// Millisecond-unit display order: the largest unit that divides evenly wins.
const MS_UNITS: &[(&str, i64)] = &[
    ("d", 86_400_000),
    ("h", 3_600_000),
    ("min", 60_000),
    ("s", 1_000),
    ("ms", 1),
];

fn invalid(name: &str, value: &str) -> CassieError {
    CassieError::InvalidParameterValue(format!(
        "invalid value for parameter \"{name}\": \"{value}\""
    ))
}

/// Canonicalizes a compatibility setting; `None` for other settings.
pub(super) fn canonical(name: &str, value: &str) -> Option<Result<String, CassieError>> {
    let trimmed = value.trim();
    Some(match name {
        "statement_timeout"
        | "lock_timeout"
        | "idle_in_transaction_session_timeout"
        | "idle_session_timeout"
        | "transaction_timeout" => milliseconds(name, trimmed).map(show_milliseconds),
        "check_function_bodies" | "row_security" => boolean(name, trimmed),
        "standard_conforming_strings" => boolean(name, trimmed).and_then(|value| {
            if value == "on" {
                Ok(value)
            } else {
                Err(CassieError::Unsupported(
                    "standard_conforming_strings = off is not supported".to_string(),
                ))
            }
        }),
        "xmloption" => match trimmed.to_ascii_lowercase().as_str() {
            value @ ("content" | "document") => Ok(value.to_string()),
            _ => Err(invalid(name, trimmed)),
        },
        "default_table_access_method" => {
            if trimmed == "heap" {
                Ok(trimmed.to_string())
            } else {
                Err(invalid(name, trimmed))
            }
        }
        "client_encoding" => client_encoding(trimmed),
        _ => return None,
    })
}

/// Parses a timeout in PostgreSQL's millisecond unit syntax.
pub(crate) fn milliseconds(name: &str, value: &str) -> Result<i64, CassieError> {
    let split = value
        .find(|character: char| character.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let (number, unit) = (value[..split].trim(), value[split..].trim());
    let scale = match unit {
        "" | "ms" => 1.0,
        "us" => 0.001,
        "s" => 1_000.0,
        "min" => 60_000.0,
        "h" => 3_600_000.0,
        "d" => 86_400_000.0,
        _ => return Err(invalid(name, value)),
    };
    let parsed = number
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or_else(|| invalid(name, value))?;
    let millis = num_traits::ToPrimitive::to_i64(&(parsed * scale).round())
        .ok_or_else(|| invalid(name, value))?;
    if !(0..=MAX_TIMEOUT_MS).contains(&millis) {
        return Err(CassieError::InvalidParameterValue(format!(
            "{millis} ms is outside the valid range for parameter \"{name}\" \
             (0 ms .. {MAX_TIMEOUT_MS} ms)"
        )));
    }
    Ok(millis)
}

/// Shows milliseconds with the largest exact unit, as PostgreSQL does.
pub(crate) fn show_milliseconds(millis: i64) -> String {
    if millis == 0 {
        return "0".to_string();
    }
    let (unit, size) = MS_UNITS
        .iter()
        .find(|(_, size)| millis % size == 0)
        .copied()
        .unwrap_or(("ms", 1));
    format!("{}{unit}", millis / size)
}

fn boolean(name: &str, value: &str) -> Result<String, CassieError> {
    crate::types::boolean::parse_text(value)
        .map(|value| if value { "on" } else { "off" }.to_string())
        .ok_or_else(|| {
            CassieError::InvalidParameterValue(format!(
                "parameter \"{name}\" requires a Boolean value"
            ))
        })
}

/// Accepts `UTF8` and its aliases. Every other encoding, including those
/// PostgreSQL supports, is rejected deterministically with `22023`.
fn client_encoding(value: &str) -> Result<String, CassieError> {
    let normalized = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
    if matches!(normalized.as_str(), "UTF8" | "UNICODE") {
        return Ok("UTF8".to_string());
    }
    Err(invalid("client_encoding", value))
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn should_canonicalize_timeouts_with_postgresql_units() {
        // Arrange
        let inputs = [
            "0", "50ms", "1234", "1s", "60000", "1500ms", "1.5s", "3600000", "86400000", "90000ms",
            "50 ms",
        ];

        // Act
        let values = inputs.map(|value| {
            canonical("statement_timeout", value)
                .and_then(Result::ok)
                .unwrap_or_default()
        });

        // Assert
        assert_eq!(
            values,
            ["0", "50ms", "1234ms", "1s", "1min", "1500ms", "1500ms", "1h", "1d", "90s", "50ms"]
        );
    }

    #[test]
    fn should_reject_invalid_timeouts_with_postgresql_messages() {
        // Arrange
        let inputs = ["abc", "-1", "5 parsecs"];

        // Act
        let messages = inputs.map(|value| {
            canonical("statement_timeout", value)
                .and_then(Result::err)
                .map(|error| error.to_string())
                .unwrap_or_default()
        });

        // Assert
        assert!(messages[0].ends_with("invalid value for parameter \"statement_timeout\": \"abc\""));
        assert!(messages[1].ends_with(
            "-1 ms is outside the valid range for parameter \"statement_timeout\" \
             (0 ms .. 2147483647 ms)"
        ));
        assert!(messages[2].contains("invalid value for parameter"));
    }

    #[test]
    fn should_resolve_client_encoding_names_given_utf8_aliases_or_other_encodings() {
        // Arrange
        let inputs = ["utf8", "UTF-8", "unicode", "LATIN1", "bogus"];

        // Act
        let outcomes = inputs.map(|value| {
            canonical("client_encoding", value).map(|result| {
                result.map_err(|error| crate::app::CassieError::descriptor(&error).sql_state)
            })
        });

        // Assert
        assert_eq!(
            outcomes,
            [
                Some(Ok("UTF8".to_string())),
                Some(Ok("UTF8".to_string())),
                Some(Ok("UTF8".to_string())),
                Some(Err("22023")),
                Some(Err("22023")),
            ]
        );
    }
}
