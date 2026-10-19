//! SQL-standard pagination syntax lowers to existing checked integer bounds.
use super::{parse_bound, Expr, SqlError};
use crate::sql::parser::{clauses, lexical};

pub(in crate::sql::parser) fn parse_fetch(sql: &str) -> Result<Option<Expr>, SqlError> {
    let sql = lexical::trim_separators(sql);
    let start = lexical::pattern_end(sql, 0, "first")
        .or_else(|| lexical::pattern_end(sql, 0, "next"))
        .ok_or_else(syntax)?;
    let rest = lexical::trim_separators(&sql[start..]);
    let (start, end) = row_marker(rest).ok_or_else(syntax)?;
    let ending = lexical::trim_separators(&rest[end..]);
    if lexical::pattern_end(ending, 0, "with ties") == Some(ending.len()) {
        return Err(SqlError::unsupported(
            "FETCH WITH TIES is not yet supported".into(),
        ));
    }
    if !ending.eq_ignore_ascii_case("only") {
        return Err(syntax());
    }
    let count = lexical::trim_separators(&rest[..start]);
    if count.is_empty() {
        return Ok(Some(Expr::IntegerLiteral(1)));
    }
    standard_count(count, false)
}

pub(in crate::sql::parser) fn parse_offset(sql: &str) -> Result<Option<Expr>, SqlError> {
    let sql = lexical::trim_separators(sql);
    if let Some((start, end)) = row_marker(sql) {
        if start > 0 && lexical::trim_separators(&sql[end..]).is_empty() {
            return standard_count(lexical::trim_separators(&sql[..start]), true);
        }
    }
    parse_bound(sql, true)
}

fn standard_count(sql: &str, offset: bool) -> Result<Option<Expr>, SqlError> {
    if sql.is_empty() || sql.eq_ignore_ascii_case("all") {
        return Err(syntax());
    }
    let expression = parse_bound(sql, offset)?.ok_or_else(syntax)?;
    // SQL-standard counts accept c_expr and signed numeric constants. An
    // operator or postfix cast needs parentheses around the complete count.
    if clauses::strip_parentheses(sql).is_none() && !standard_atom(sql, &expression) {
        return Err(syntax());
    }
    Ok(Some(expression))
}

fn standard_atom(sql: &str, expression: &Expr) -> bool {
    if matches!(sql.as_bytes().first(), Some(b'+' | b'-')) {
        let operand = lexical::trim_separators(&sql[1..]);
        return operand
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            && matches!(
                expression,
                Expr::IntegerLiteral(_) | Expr::NumberLiteral(_) | Expr::NumericLiteral(_)
            );
    }
    matches!(
        expression,
        Expr::IntegerLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::NumericLiteral(_)
            | Expr::StringLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null
            | Expr::Param(_)
            | Expr::Column(_)
            | Expr::Function(_)
            | Expr::Case { .. }
            | Expr::Exists(_)
            | Expr::Subquery(_)
    )
}

pub(in crate::sql::parser) fn validate_fetch_order(
    matches: &[clauses::ClauseMatch],
) -> Result<(), SqlError> {
    use clauses::{Clause, ClauseToken};
    if !matches
        .iter()
        .any(|clause| matches!(clause.token, ClauseToken::Recognized(Clause::Fetch)))
    {
        return Ok(());
    }
    let mut bounds = false;
    for clause in matches {
        if matches!(
            clause.token,
            ClauseToken::Recognized(Clause::Fetch | Clause::Offset | Clause::Limit)
        ) {
            bounds = true;
        } else if bounds {
            return Err(syntax());
        }
    }
    Ok(())
}

fn row_marker(sql: &str) -> Option<(usize, usize)> {
    let mut last = None;
    for token in ["row", "rows"] {
        let mut cursor = 0;
        while let Some(span) = clauses::find_top_level_keyword_span(sql, cursor, token) {
            if last.is_none_or(|(position, _)| span.0 > position) {
                last = Some(span);
            }
            cursor = span.1;
        }
    }
    last
}

fn syntax() -> SqlError {
    SqlError::syntax("invalid SQL-standard pagination syntax".into())
}

#[cfg(test)]
#[path = "pagination_fetch_tests.rs"]
mod tests;
