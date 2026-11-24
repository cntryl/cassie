//! SQL-standard value keywords and date/time syntax: `CURRENT_CATALOG`,
//! `CURRENT_DATE`-style keywords, `EXTRACT`,
//! interval field qualifiers, `AT TIME ZONE`/`AT LOCAL`, `OVERLAPS` and
//! `make_interval` named arguments, each lowered to a function call.
use super::super::clauses::{split_top_level, strip_parentheses};
use super::super::lexical;
use super::qualified_operators::Index;
use super::{
    parse_expression, parse_prefix_expression_indexed, split_csv, Expr, FunctionCall, SqlError,
};

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Function(Box::new(FunctionCall {
        name: name.to_string(),
        args,
        quantifier: crate::sql::ast::AggregateQuantifier::Default,
        filter: None,
        order_by: Vec::new(),
    }))
}

const SQL_VALUE_KEYWORDS: [&str; 9] = [
    "current_catalog",
    "current_date",
    "current_role",
    "current_time",
    "current_timestamp",
    "current_user",
    "localtime",
    "localtimestamp",
    "session_user",
];

/// Parses a temporal expression token; `None` when `raw` is not one.
pub(super) fn token(raw: &str) -> Result<Option<Expr>, SqlError> {
    let lower = raw.to_ascii_lowercase();
    if SQL_VALUE_KEYWORDS.contains(&lower.as_str()) {
        return Ok(Some(call(&lower, Vec::new())));
    }
    if let Some(expr) = extract(raw, &lower)? {
        return Ok(Some(expr));
    }
    if let Some(expr) = make_interval(raw, &lower)? {
        return Ok(Some(expr));
    }
    qualified_interval(raw, &lower)
}

fn extract(raw: &str, lower: &str) -> Result<Option<Expr>, SqlError> {
    let Some(rest) = lower.strip_prefix("extract") else {
        return Ok(None);
    };
    if !rest.trim_start().starts_with('(') {
        return Ok(None);
    }
    let offset = raw.len() - rest.trim_start().len();
    let Some(inner) = strip_parentheses(&raw[offset..]) else {
        return Ok(None);
    };
    let (field, source) = split_top_level(inner, " from ")
        .ok_or_else(|| SqlError::syntax("EXTRACT requires FROM".into()))?;
    let field = field.trim();
    let field = field
        .strip_prefix('\'')
        .and_then(|field| field.strip_suffix('\''))
        .or_else(|| {
            field
                .strip_prefix('"')
                .and_then(|field| field.strip_suffix('"'))
        })
        .unwrap_or(field);
    Ok(Some(call(
        "extract",
        vec![
            Expr::StringLiteral(field.to_ascii_lowercase()),
            parse_expression(source)?,
        ],
    )))
}

const MAKE_INTERVAL_FIELDS: [&str; 7] =
    ["years", "months", "weeks", "days", "hours", "mins", "secs"];

fn make_interval(raw: &str, lower: &str) -> Result<Option<Expr>, SqlError> {
    let Some(rest) = lower.strip_prefix("make_interval") else {
        return Ok(None);
    };
    if !rest.trim_start().starts_with('(') || !raw.contains("=>") {
        return Ok(None);
    }
    let offset = raw.len() - rest.trim_start().len();
    let Some(inner) = strip_parentheses(&raw[offset..]) else {
        return Ok(None);
    };
    let mut args: Vec<Expr> = MAKE_INTERVAL_FIELDS
        .iter()
        .map(|field| {
            if *field == "secs" {
                Expr::NumberLiteral(0.0)
            } else {
                Expr::IntegerLiteral(0)
            }
        })
        .collect();
    for (position, argument) in split_csv(inner).into_iter().enumerate() {
        if let Some((name, value)) = argument.split_once("=>") {
            let name = name.trim().to_ascii_lowercase();
            let index = MAKE_INTERVAL_FIELDS
                .iter()
                .position(|field| *field == name)
                .ok_or_else(|| {
                    SqlError::new(format!("function make_interval has no argument \"{name}\""))
                })?;
            args[index] = parse_expression(value)?;
        } else if position < args.len() {
            args[position] = parse_expression(argument)?;
        }
    }
    Ok(Some(call("make_interval", args)))
}

/// `INTERVAL 'text' HOUR TO MINUTE` and `INTERVAL 'text' SECOND(3)`.
fn qualified_interval(raw: &str, lower: &str) -> Result<Option<Expr>, SqlError> {
    if !lower.starts_with("interval") {
        return Ok(None);
    }
    let Some(quote) = raw.find('\'') else {
        return Ok(None);
    };
    let Some(end) = lexical::quoted_end(raw, quote) else {
        return Ok(None);
    };
    let qualifier = raw[end..].trim();
    if qualifier.is_empty() {
        return Ok(None);
    }
    let type_name = format!("{} {qualifier}", raw[..quote].trim());
    let Ok(data_type) = crate::types::DataType::parse_sql(&type_name) else {
        return Ok(None);
    };
    let Expr::StringLiteral(text) = super::parse_expr_token(&raw[quote..end])? else {
        return Ok(None);
    };
    Ok(Some(Expr::Cast {
        expr: Box::new(Expr::StringLiteral(text)),
        data_type,
        source_type: None,
        identity: crate::sql::ast::CastIdentity::Scalar,
    }))
}

/// Finds the last top-level occurrence of `keyword`.
fn last_top_level(raw: &str, keyword: &str) -> Option<(usize, usize)> {
    let mut found = None;
    let mut cursor = 0;
    while let Some((start, end)) = lexical::find_top_level_span(raw, cursor, keyword) {
        found = Some((start, end));
        cursor = end;
    }
    found
}

/// `value AT TIME ZONE zone` → `timezone(zone, value)`; `value AT LOCAL` →
/// `timezone(value)`.
pub(super) fn at_time_zone(raw: &str, operators: &Index<'_>) -> Result<Option<Expr>, SqlError> {
    let zone = last_top_level(raw, " at time zone ");
    let local = last_top_level(raw, " at local");
    let use_local = match (zone, local) {
        (None, None) => return Ok(None),
        (Some((zone_start, _)), Some((local_start, _))) => local_start > zone_start,
        (None, Some(_)) => true,
        (Some(_), None) => false,
    };
    if use_local {
        let (start, end) = local.unwrap_or_default();
        if !raw[end..].trim().is_empty() {
            return Ok(None);
        }
        return Ok(Some(call(
            "timezone",
            vec![parse_prefix_expression_indexed(&raw[..start], operators)?],
        )));
    }
    let (start, end) = zone.unwrap_or_default();
    Ok(Some(call(
        "timezone",
        vec![
            parse_prefix_expression_indexed(&raw[end..], operators)?,
            parse_prefix_expression_indexed(&raw[..start], operators)?,
        ],
    )))
}

/// `(start, end_or_length) OVERLAPS (start, end_or_length)`.
pub(super) fn overlaps(raw: &str) -> Result<Option<Expr>, SqlError> {
    let Some((left, right)) = split_top_level(raw, " overlaps ") else {
        return Ok(None);
    };
    let pair = |side: &str| -> Result<Vec<Expr>, SqlError> {
        let inner = strip_parentheses(side.trim())
            .ok_or_else(|| SqlError::syntax("OVERLAPS requires row constructors".into()))?;
        let items = split_csv(inner);
        if items.len() != 2 {
            return Err(SqlError::syntax(
                "OVERLAPS requires two-element row constructors".into(),
            ));
        }
        items.into_iter().map(parse_expression).collect()
    };
    let mut args = pair(left)?;
    args.extend(pair(right)?);
    Ok(Some(call("overlaps", args)))
}
