//! `ROW(...)`, implicit `(a, b)` and `(expr).field` expression syntax.
//!
//! Both forms parse to internal marker functions, like `ARRAY[...]`, so the
//! expression tree gains no new variants.
use super::super::lexical;
use super::{parse_expression, row_predicates, Expr, SqlError};
use crate::sql::ast::{FunctionCall, FIELD_ACCESS_FUNCTION, ROW_CONSTRUCTOR_FUNCTION};

/// `ROW(a, b)` and `(a, b)` outside a row predicate.
pub(super) fn row_constructor(raw: &str) -> Result<Option<Expr>, SqlError> {
    let Some(args) = row_predicates::constructor(raw)? else {
        return Ok(None);
    };
    Ok(Some(Expr::Function(Box::new(FunctionCall {
        name: ROW_CONSTRUCTOR_FUNCTION.into(),
        args,
        ..FunctionCall::default()
    }))))
}

/// `(expr).field`, optionally chained as `(expr).a.b`.
pub(super) fn field_access(raw: &str) -> Result<Option<Expr>, SqlError> {
    let raw = lexical::trim_separators(raw);
    if !raw.starts_with('(') {
        return Ok(None);
    }
    let Some(close) = lexical::matching_paren(raw, 0) else {
        return Ok(None);
    };
    let mut rest = lexical::trim_separators(&raw[close + 1..]);
    if !rest.starts_with('.') {
        return Ok(None);
    }
    let inner = lexical::trim_separators(&raw[1..close]);
    if inner.is_empty() {
        return Ok(None);
    }
    let mut expr = parse_expression(inner)?;
    while let Some(after_dot) = rest.strip_prefix('.') {
        let after_dot = lexical::trim_separators(after_dot);
        let (field, remainder) = take_field_name(after_dot)?;
        expr = Expr::Function(Box::new(FunctionCall {
            name: FIELD_ACCESS_FUNCTION.into(),
            args: vec![expr, Expr::StringLiteral(field)],
            ..FunctionCall::default()
        }));
        rest = lexical::trim_separators(remainder);
    }
    if rest.is_empty() {
        Ok(Some(expr))
    } else {
        Ok(None)
    }
}

fn take_field_name(raw: &str) -> Result<(String, &str), SqlError> {
    if raw.starts_with('*') {
        return Err(SqlError::unsupported(
            "(expression).* expansion is not supported for composite values".into(),
        ));
    }
    if let Some(body) = raw.strip_prefix('"') {
        let Some((decoded, consumed)) = crate::sql::identifier_text::quoted(
            body,
            crate::sql::identifier_text::IDENTIFIER_BYTES,
        ) else {
            return Err(SqlError::syntax("unterminated quoted identifier".into()));
        };
        return Ok((decoded, &body[consumed..]));
    }
    let end = raw
        .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$'))
        .unwrap_or(raw.len());
    if end == 0 {
        return Err(SqlError::syntax("expected a field name after \".\"".into()));
    }
    Ok((raw[..end].to_ascii_lowercase(), &raw[end..]))
}
