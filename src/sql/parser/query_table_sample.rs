use crate::sql::ast::TableSampleMethod;

use super::{Expr, SqlError};

pub(super) fn parse_table_sample_clause(
    raw: &str,
) -> Result<(TableSampleMethod, Expr, Option<Expr>, &str), SqlError> {
    let raw = super::super::lexical::trim_separators(raw);
    let method_end = raw
        .find(|character: char| character.is_whitespace() || character == '(')
        .unwrap_or(raw.len());
    let method = match raw[..method_end].to_ascii_lowercase().as_str() {
        "bernoulli" => TableSampleMethod::Bernoulli,
        "system" => TableSampleMethod::System,
        "" => return Err(SqlError::syntax("TABLESAMPLE requires a method".into())),
        _ => return Err(SqlError::syntax("unsupported TABLESAMPLE method".into())),
    };
    let arguments = super::super::lexical::trim_separators(&raw[method_end..]);
    if !arguments.starts_with('(') {
        return Err(SqlError::syntax(
            "TABLESAMPLE method requires a parenthesized rate".into(),
        ));
    }
    let close = super::super::lexical::matching_paren(arguments, 0)
        .ok_or_else(|| SqlError::syntax("unterminated TABLESAMPLE rate".into()))?;
    let rate_raw = arguments[1..close].trim();
    if rate_raw.is_empty() {
        return Err(SqlError::syntax("TABLESAMPLE rate cannot be empty".into()));
    }
    let rate = super::super::expr::parse_expression(rate_raw)?;
    let remainder = super::super::lexical::trim_separators(&arguments[close + 1..]);
    let repeatable_end = super::super::lexical::pattern_end(remainder, 0, "repeatable");
    let (repeatable, alias_tail) = if let Some(end) = repeatable_end {
        let seed = super::super::lexical::trim_separators(&remainder[end..]);
        if !seed.starts_with('(') {
            return Err(SqlError::syntax(
                "REPEATABLE requires a parenthesized seed".into(),
            ));
        }
        let close = super::super::lexical::matching_paren(seed, 0)
            .ok_or_else(|| SqlError::syntax("unterminated REPEATABLE seed".into()))?;
        let seed_raw = seed[1..close].trim();
        if seed_raw.is_empty() {
            return Err(SqlError::syntax("REPEATABLE seed cannot be empty".into()));
        }
        (
            Some(super::super::expr::parse_expression(seed_raw)?),
            super::super::lexical::trim_separators(&seed[close + 1..]),
        )
    } else {
        (None, remainder)
    };
    Ok((method, rate, repeatable, alias_tail))
}
