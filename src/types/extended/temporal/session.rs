//! Session settings that shape temporal input and output.
use super::zone::{AbbreviationSet, Zone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateOrder {
    #[default]
    Mdy,
    Dmy,
    Ymd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateOutput {
    #[default]
    Iso,
    Sql,
    Postgres,
    German,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntervalStyle {
    #[default]
    Postgres,
    PostgresVerbose,
    SqlStandard,
    Iso8601,
}

impl IntervalStyle {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "postgres" => Some(Self::Postgres),
            "postgres_verbose" => Some(Self::PostgresVerbose),
            "sql_standard" => Some(Self::SqlStandard),
            "iso_8601" => Some(Self::Iso8601),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::PostgresVerbose => "postgres_verbose",
            Self::SqlStandard => "sql_standard",
            Self::Iso8601 => "iso_8601",
        }
    }
}

/// Parses a `DateStyle` value against the current style, as `check_datestyle`
/// does: each comma-separated word sets the output style or the field order.
///
/// # Errors
/// Returns the offending word when it is not a recognized keyword or when two
/// words conflict.
pub fn parse_datestyle(
    value: &str,
    current: (DateOutput, DateOrder),
) -> Result<(DateOutput, DateOrder), String> {
    let (mut output, mut order) = current;
    let (mut seen_output, mut seen_order) = (None, None);
    for word in value
        .split([',', ' '])
        .map(str::trim)
        .filter(|word| !word.is_empty())
    {
        let word = word.trim_matches('"').to_ascii_lowercase();
        let (new_output, new_order) = match word.as_str() {
            "iso" => (Some(DateOutput::Iso), None),
            "sql" => (Some(DateOutput::Sql), None),
            "postgres" => (Some(DateOutput::Postgres), None),
            "german" => (Some(DateOutput::German), Some(DateOrder::Dmy)),
            "ymd" => (None, Some(DateOrder::Ymd)),
            "dmy" | "euro" | "european" => (None, Some(DateOrder::Dmy)),
            "mdy" | "us" | "noneuro" | "noneuropean" => (None, Some(DateOrder::Mdy)),
            "default" => (Some(DateOutput::Iso), Some(DateOrder::Mdy)),
            _ => return Err(word),
        };
        if let Some(new_output) = new_output {
            if seen_output.is_some_and(|seen| seen != new_output) {
                return Err(value.to_string());
            }
            seen_output = Some(new_output);
            output = new_output;
            if word == "german" && seen_order.is_none() {
                order = DateOrder::Dmy;
            }
        }
        if let Some(new_order) = new_order.filter(|_| word != "german") {
            if seen_order.is_some_and(|seen| seen != new_order) {
                return Err(value.to_string());
            }
            seen_order = Some(new_order);
            order = new_order;
        }
    }
    Ok((output, order))
}

#[must_use]
pub fn datestyle_text(output: DateOutput, order: DateOrder) -> String {
    let output = match output {
        DateOutput::Iso => "ISO",
        DateOutput::Sql => "SQL",
        DateOutput::Postgres => "Postgres",
        DateOutput::German => "German",
    };
    let order = match order {
        DateOrder::Mdy => "MDY",
        DateOrder::Dmy => "DMY",
        DateOrder::Ymd => "YMD",
    };
    format!("{output}, {order}")
}

/// The temporal view of one statement's session.
#[derive(Debug, Clone)]
pub struct TemporalSession {
    pub zone: Zone,
    pub date_output: DateOutput,
    pub date_order: DateOrder,
    pub interval_style: IntervalStyle,
    pub abbreviations: AbbreviationSet,
    /// `transaction_timestamp()`, microseconds since 2000-01-01 UTC.
    pub transaction_start: i64,
    /// `statement_timestamp()`.
    pub statement_start: i64,
}

impl Default for TemporalSession {
    fn default() -> Self {
        let now = current_timestamp();
        Self {
            zone: Zone::utc(),
            date_output: DateOutput::Iso,
            date_order: DateOrder::Mdy,
            interval_style: IntervalStyle::Postgres,
            abbreviations: AbbreviationSet::Default,
            transaction_start: now,
            statement_start: now,
        }
    }
}

/// The wall clock as a PostgreSQL timestamp (microseconds since 2000-01-01).
#[must_use]
pub fn current_timestamp() -> i64 {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let micros = i64::try_from(elapsed.as_micros()).unwrap_or(i64::MAX);
    micros - super::calendar::UNIX_TO_POSTGRES_SECS * super::calendar::USECS_PER_SEC
}

#[cfg(test)]
mod tests {
    use super::{parse_datestyle, DateOrder, DateOutput};

    #[test]
    fn should_parse_datestyle_words_like_postgresql() {
        // Arrange
        let current = (DateOutput::Iso, DateOrder::Mdy);

        // Act
        let sql = parse_datestyle("SQL, DMY", current);
        let german = parse_datestyle("German", current);
        let bad = parse_datestyle("ISO, SQL", current);

        // Assert
        assert_eq!(sql, Ok((DateOutput::Sql, DateOrder::Dmy)));
        assert_eq!(german, Ok((DateOutput::German, DateOrder::Dmy)));
        assert!(bad.is_err());
    }
}
