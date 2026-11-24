//! Positional and named arguments of `graph_weighted_paths` (#1258).
//!
//! The call is rewritten to its nine positional arguments: named arguments move to their
//! slots and omitted optional ones take their declared defaults, so an explicit `NULL`
//! stays distinct from an omission.

use super::expr::{parse_expression, split_csv};
use super::identifiers::normalize_identifier;
use super::{Expr, SqlError};
use crate::registry::functions::graph_weighted::{ARGUMENTS, REQUIRED};

fn split_named(raw: &str) -> Option<(&str, &str)> {
    super::clauses::find_top_level_keyword_span(raw, 0, "=>")
        .map(|(position, end)| (raw[..position].trim(), raw[end..].trim()))
}

fn default_of(slot: usize) -> Expr {
    match slot {
        5 => Expr::StringLiteral("out".to_owned()),
        8 => Expr::IntegerLiteral(1),
        _ => Expr::Null,
    }
}

fn value(raw: &str) -> Result<Expr, SqlError> {
    if raw.trim().is_empty() {
        return Err(SqlError::syntax("missing function argument".into()));
    }
    parse_expression(raw)
}

fn no_match() -> SqlError {
    SqlError::undefined_function(format!(
        "function {}(...) does not exist",
        crate::registry::functions::graph_weighted::NAME
    ))
}

/// The nine positional arguments of a `graph_weighted_paths` call, `None` for any other.
///
/// # Errors
///
/// `42601` for a duplicate name or a positional argument after a named one, and `42883`
/// for an unknown name, too many arguments or a missing required argument.
pub(super) fn arguments(name: &str, body: &str) -> Result<Option<Vec<Expr>>, SqlError> {
    if !crate::registry::functions::graph_weighted::is_weighted_paths(name) {
        return Ok(None);
    }
    let mut slots: Vec<Option<Expr>> = vec![None; ARGUMENTS.len()];
    let mut named = false;
    let raw = split_csv(body);
    for (position, argument) in raw.into_iter().enumerate() {
        if let Some((label, text)) = split_named(argument) {
            named = true;
            let label = normalize_identifier(label)?;
            let slot = ARGUMENTS
                .iter()
                .position(|expected| *expected == label)
                .ok_or_else(no_match)?;
            if slots[slot].is_some() {
                return Err(SqlError::syntax(format!(
                    "argument name \"{label}\" used more than once"
                )));
            }
            slots[slot] = Some(value(text)?);
            continue;
        }
        if named {
            return Err(SqlError::syntax(
                "positional argument cannot follow named argument".into(),
            ));
        }
        let slot = slots.get_mut(position).ok_or_else(no_match)?;
        *slot = Some(value(argument)?);
    }
    if slots[..REQUIRED].iter().any(Option::is_none) {
        return Err(no_match());
    }
    Ok(Some(
        slots
            .into_iter()
            .enumerate()
            .map(|(slot, expr)| expr.unwrap_or_else(|| default_of(slot)))
            .collect(),
    ))
}
