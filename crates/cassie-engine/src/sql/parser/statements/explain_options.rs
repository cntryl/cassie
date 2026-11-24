//! The `EXPLAIN` option list and its legacy `ANALYZE`/`VERBOSE` keywords,
//! validated as PostgreSQL 18 validates them.
//!
//! `ANALYZE` executes the statement. The remaining options are accepted for
//! compatibility and leave Cassie's single text plan format unchanged;
//! `FORMAT JSON`, `YAML` and `XML` are rejected as unsupported.

use crate::sql::parser::{expr::split_csv, lexical, SqlError};

/// The options that change execution, and the statement text they precede.
pub(in crate::sql::parser) struct ExplainPrefix<'a> {
    pub(in crate::sql::parser) analyze: bool,
    pub(in crate::sql::parser) statement: &'a str,
}

const BOOLEAN_OPTIONS: &[&str] = &[
    "analyze",
    "verbose",
    "costs",
    "settings",
    "buffers",
    "wal",
    "timing",
    "summary",
    "generic_plan",
    "memory",
];

/// Splits `EXPLAIN`'s options from its statement; `rest` follows `EXPLAIN`.
pub(in crate::sql::parser) fn parse_explain_prefix(
    rest: &str,
) -> Result<ExplainPrefix<'_>, SqlError> {
    let rest = lexical::trim_separators(rest);
    if rest.starts_with('(') && !parenthesized_query(&rest[1..]) {
        let close = lexical::matching_paren(rest, 0)
            .ok_or_else(|| SqlError::syntax("unterminated EXPLAIN option list".into()))?;
        let analyze = parse_option_list(&rest[1..close])?;
        return Ok(ExplainPrefix {
            analyze,
            statement: lexical::trim_separators(&rest[close + 1..]),
        });
    }
    let mut statement = rest;
    let mut analyze = false;
    for keyword in ["ANALYZE ", "ANALYSE ", "VERBOSE "] {
        if let Some(end) = lexical::pattern_end(statement, 0, keyword) {
            analyze |= !keyword.starts_with('V');
            statement = &statement[end..];
        }
    }
    Ok(ExplainPrefix { analyze, statement })
}

/// Whether `(` opens a parenthesized query rather than an option list.
fn parenthesized_query(inner: &str) -> bool {
    let inner = lexical::trim_separators(inner);
    inner.starts_with('(')
        || ["SELECT", "VALUES", "WITH", "TABLE"]
            .iter()
            .any(|keyword| lexical::pattern_end(inner, 0, keyword).is_some())
}

fn parse_option_list(list: &str) -> Result<bool, SqlError> {
    if list.trim().is_empty() {
        return Err(SqlError::syntax("syntax error at or near \")\"".into()));
    }
    let mut analyze = false;
    let mut explicit = Vec::<(&'static str, bool)>::new();
    let mut serialize = false;
    for option in split_csv(list) {
        let option = option.trim();
        let (name, value) = option
            .split_once(char::is_whitespace)
            .map_or((option, None), |(name, value)| (name, Some(value.trim())));
        let name = name.to_ascii_lowercase();
        if let Some(known) = BOOLEAN_OPTIONS.iter().find(|known| **known == name) {
            let enabled = value.map_or(Ok(true), |value| boolean(&name, value))?;
            explicit.retain(|(option, _)| option != known);
            explicit.push((known, enabled));
            if name == "analyze" {
                analyze = enabled;
            }
        } else if name == "format" {
            match value.map(unquote).as_deref() {
                Some("text") => {}
                Some(format @ ("json" | "yaml" | "xml")) => {
                    return Err(SqlError::unsupported(format!(
                        "EXPLAIN FORMAT {} is not supported",
                        format.to_ascii_uppercase()
                    )))
                }
                Some(other) => return Err(invalid_value("format", other)),
                None => {
                    return Err(SqlError::syntax(
                        "EXPLAIN option format requires a value".into(),
                    ))
                }
            }
        } else if name == "serialize" {
            serialize = match value.map(unquote).as_deref() {
                None | Some("text" | "binary") => true,
                Some("none") => false,
                Some(other) => return Err(invalid_value("serialize", other)),
            };
        } else {
            return Err(SqlError::syntax(format!(
                "unrecognized EXPLAIN option \"{name}\""
            )));
        }
    }
    let enabled = |option: &str| {
        explicit
            .iter()
            .any(|(name, value)| *name == option && *value)
    };
    for option in ["timing", "wal"] {
        if enabled(option) && !analyze {
            return Err(SqlError::invalid_parameter_value(format!(
                "EXPLAIN option {} requires ANALYZE",
                option.to_ascii_uppercase()
            )));
        }
    }
    if serialize && !analyze {
        return Err(SqlError::invalid_parameter_value(
            "EXPLAIN option SERIALIZE requires ANALYZE".into(),
        ));
    }
    if analyze && enabled("generic_plan") {
        return Err(SqlError::invalid_parameter_value(
            "EXPLAIN options ANALYZE and GENERIC_PLAN cannot be used together".into(),
        ));
    }
    Ok(analyze)
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn boolean(name: &str, value: &str) -> Result<bool, SqlError> {
    crate::types::boolean::parse_text(&unquote(value))
        .ok_or_else(|| SqlError::syntax(format!("{name} requires a Boolean value")))
}

fn invalid_value(option: &str, value: &str) -> SqlError {
    SqlError::invalid_parameter_value(format!(
        "unrecognized value for EXPLAIN option \"{option}\": \"{value}\""
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_explain_prefix;
    use crate::sql::parser::SqlErrorKind;

    #[test]
    fn should_accept_postgresql_explain_options_given_explicit_booleans() {
        // Arrange
        let inputs = [
            "(ANALYZE FALSE, VERBOSE TRUE, COSTS off, SETTINGS, BUFFERS on, SUMMARY, MEMORY, FORMAT TEXT) SELECT 1",
            "(ANALYZE, ANALYZE false) SELECT 1",
            "(ANALYZE 1, SERIALIZE TEXT, TIMING) SELECT 1",
            "(COSTS 'off', GENERIC_PLAN) SELECT $1",
            "ANALYZE VERBOSE SELECT 1",
            "VERBOSE SELECT 1",
            "(SELECT 1) UNION SELECT 2",
        ];

        // Act
        let parsed = inputs.map(|sql| {
            parse_explain_prefix(sql)
                .ok()
                .map(|prefix| (prefix.analyze, prefix.statement.to_string()))
        });

        // Assert
        assert_eq!(
            parsed,
            [
                Some((false, "SELECT 1".to_string())),
                Some((false, "SELECT 1".to_string())),
                Some((true, "SELECT 1".to_string())),
                Some((false, "SELECT $1".to_string())),
                Some((true, "SELECT 1".to_string())),
                Some((false, "SELECT 1".to_string())),
                Some((false, "(SELECT 1) UNION SELECT 2".to_string())),
            ]
        );
    }

    #[test]
    fn should_reject_invalid_explain_options_with_postgresql_error_classes() {
        // Arrange
        let inputs = [
            "(FOO) SELECT 1",
            "(ANALYZE maybe) SELECT 1",
            "(FORMAT CSV) SELECT 1",
            "(TIMING) SELECT 1",
            "(GENERIC_PLAN, ANALYZE) SELECT 1",
            "(SERIALIZE) SELECT 1",
            "(WAL) SELECT 1",
            "() SELECT 1",
            "(FORMAT JSON) SELECT 1",
        ];

        // Act
        let kinds = inputs.map(|sql| parse_explain_prefix(sql).err().map(|error| error.kind()));

        // Assert
        assert_eq!(
            kinds,
            [
                Some(SqlErrorKind::Syntax),
                Some(SqlErrorKind::Syntax),
                Some(SqlErrorKind::InvalidParameterValue),
                Some(SqlErrorKind::InvalidParameterValue),
                Some(SqlErrorKind::InvalidParameterValue),
                Some(SqlErrorKind::InvalidParameterValue),
                Some(SqlErrorKind::InvalidParameterValue),
                Some(SqlErrorKind::Syntax),
                Some(SqlErrorKind::Unsupported),
            ]
        );
    }
}
