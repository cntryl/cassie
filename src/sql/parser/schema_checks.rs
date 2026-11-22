//! Selected simple field/literal CHECK predicates.
use super::super::lexical::{find_top_level_span, trim_separators};
use super::{
    parse_constant_literal, strip_parentheses, ConstraintCheck, ConstraintOperator, SqlError, Value,
};

pub(super) fn parse_check_constraint(raw: &str) -> Result<ConstraintCheck, SqlError> {
    let expression = raw.trim();
    let upper = expression.to_ascii_uppercase();
    if upper.ends_with(" NOT ENFORCED") || upper.ends_with(" ENFORCED") {
        return Err(SqlError::unsupported(
            "CHECK constraint enforcement attributes are not supported".into(),
        ));
    }
    if upper.ends_with(" NOT VALID") {
        return Err(SqlError::unsupported(
            "NOT VALID CHECK constraints are not supported".into(),
        ));
    }
    if !expression.starts_with('(') || !expression.ends_with(')') {
        return Err(SqlError::new(
            "CHECK expression must be parenthesized".to_string(),
        ));
    }
    let inner = strip_parentheses(expression)
        .ok_or_else(|| SqlError::new("invalid CHECK expression".to_string()))?
        .trim();

    crate::sql::definition_guard::expression_sql(inner)
        .map_err(|error| SqlError::unsupported(error.to_string()))?;

    let (left, op, right) = parse_simple_comparison(inner).ok_or_else(|| {
        non_finite_identifier(inner)
            .unwrap_or_else(|| SqlError::new("unsupported CHECK expression".to_string()))
    })?;

    if left.starts_with('(') {
        return Err(SqlError::unsupported(
            "a CHECK constraint on a field of a composite column is not supported".to_string(),
        ));
    }
    let field = resolve_check_field(&left)?;

    Ok(ConstraintCheck {
        field,
        operator: op,
        value: right,
        bound_expression: None,
    })
}

/// Resolves the checked column. A double-quoted identifier keeps its exact
/// spelling through the shared column identifier rules; an unquoted name keeps
/// the historical identifier-or-dotted-path check.
fn resolve_check_field(left: &str) -> Result<String, SqlError> {
    if left.starts_with('"') {
        let path = crate::sql::ColumnIdentifierPath::parse_sql(left).map_err(SqlError::new)?;
        if path.is_qualified() {
            return Err(SqlError::unsupported(
                "a CHECK constraint must name a single column of this table".to_string(),
            ));
        }
        return Ok(path.lookup_key());
    }
    if !left
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(SqlError::new(
            "CHECK expression field must be an identifier".to_string(),
        ));
    }
    Ok(left.to_string())
}

/// SQL reads an unquoted `NaN`, `Infinity` or `inf` (optionally signed) as a
/// column reference, so PostgreSQL rejects it with `42703`. The quoted forms
/// `'NaN'` and `'Infinity'::float8` are the way to write a non-finite bound.
fn non_finite_identifier(expression: &str) -> Option<SqlError> {
    comparison_splits(expression)
        .into_iter()
        .find_map(|(_, _, right)| {
            let word = right.trim().trim_start_matches(['+', '-']).trim();
            ["nan", "inf", "infinity"]
                .iter()
                .any(|name| word.eq_ignore_ascii_case(name))
                .then(|| {
                    SqlError::diagnostic(
                        "42703",
                        format!("column \"{}\" does not exist", word.to_ascii_lowercase()),
                    )
                })
        })
}

fn parse_simple_comparison(raw: &str) -> Option<(String, ConstraintOperator, Value)> {
    comparison_splits(raw)
        .into_iter()
        .find_map(|(left, kind, right)| {
            parse_constant_literal(right)
                .ok()
                .map(|value| (left.to_string(), kind, value))
        })
}

/// Every way `raw` splits at one top-level comparison operator, matching
/// overlapping symbols longest-first. The shared scanner skips strings,
/// delimited identifiers, comments, and nested expressions.
fn comparison_splits(raw: &str) -> Vec<(&str, ConstraintOperator, &str)> {
    let candidates = [
        (
            "is not distinct from",
            ConstraintOperator::IsNotDistinctFrom,
        ),
        ("is distinct from", ConstraintOperator::IsDistinctFrom),
        ("<=", ConstraintOperator::Lte),
        (">=", ConstraintOperator::Gte),
        ("<>", ConstraintOperator::NotEq),
        ("!=", ConstraintOperator::NotEq),
        ("like", ConstraintOperator::Like),
        ("<", ConstraintOperator::Lt),
        (">", ConstraintOperator::Gt),
        ("=", ConstraintOperator::Eq),
    ];
    candidates
        .into_iter()
        .filter_map(|(operator, kind)| {
            let (start, end) = find_top_level_span(raw, 0, operator)?;
            let left = trim_separators(&raw[..start]);
            (!left.is_empty()).then(|| (left, kind, trim_separators(&raw[end..])))
        })
        .collect()
}
