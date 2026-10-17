//! Resolve admitted qualified builtin spellings at generic operator precedence.
use super::depth;
use crate::sql::ast::{BinaryOp, UnaryNumericOp};
use crate::sql::SqlError;
use crate::types::IntegerOperation;

#[derive(Clone, Copy)]
pub(super) struct Span<'a> {
    pub start: usize,
    pub end: usize,
    symbol: &'a str,
}

fn token(raw: &str, start: usize, keyword_end: usize) -> Result<Option<Span<'_>>, SqlError> {
    let open = keyword_end + super::super::lexical::separator_end(&raw[keyword_end..], 0);
    if !raw[open..].starts_with('(') {
        return Ok(None);
    }
    let close = super::super::lexical::matching_paren(raw, open)
        .ok_or_else(|| SqlError::syntax("unterminated qualified operator".into()))?;
    let body = super::super::lexical::trim_separators(&raw[open + 1..close]);
    let (dot, end) =
        super::super::clauses::find_top_level_keyword_span(body, 0, ".").ok_or_else(|| {
            SqlError::unsupported("unqualified OPERATOR lookup remains pending".into())
        })?;
    let schema = crate::sql::ColumnIdentifierPath::parse_sql(
        super::super::lexical::trim_separators(&body[..dot]),
    )
    .map_err(SqlError::syntax)?;
    let symbol = super::super::lexical::trim_separators(&body[end..]);
    if symbol.is_empty()
        || !symbol
            .chars()
            .all(super::super::operators::operator_character)
    {
        return Err(SqlError::syntax("invalid qualified operator symbol".into()));
    }
    if schema.is_qualified() || schema.lookup_key() != "pg_catalog" {
        return Err(SqlError::unsupported(
            "qualified operator namespace is outside the selected builtin contract".into(),
        ));
    }
    Ok(Some(Span {
        start,
        end: close + 1,
        symbol,
    }))
}

pub(super) fn last_binary(raw: &str) -> Result<Option<Span<'_>>, SqlError> {
    let mut cursor = 0;
    let mut previous_end = None;
    let mut selected = None;
    let mut count = 0;
    while let Some((start, end)) =
        super::super::clauses::find_top_level_keyword_span(raw, cursor, "operator")
    {
        // A member named operator is an identifier, as in public.operator(2).
        if super::super::lexical::trim_separators(&raw[..start]).ends_with('.') {
            cursor = end;
            continue;
        }
        let Some(span) = token(raw, start, end)? else {
            cursor = end;
            continue;
        };
        count += 1;
        if count > depth::MAX_POSTFIX_DEPTH {
            return Err(depth::limit_error());
        }
        let left = super::super::lexical::trim_separators(&raw[..start]);
        let adjacent = previous_end
            .is_some_and(|end| super::super::lexical::trim_separators(&raw[end..start]).is_empty());
        if !left.is_empty()
            && !left.ends_with(super::super::operators::operator_character)
            && !adjacent
        {
            selected = Some(span);
        }
        previous_end = Some(span.end);
        cursor = span.end;
    }
    Ok(selected)
}

pub(super) fn prefix(raw: &str) -> Result<Option<(&str, UnaryNumericOp)>, SqlError> {
    let Some(end) = super::super::lexical::pattern_end(raw, 0, "operator") else {
        return Ok(None);
    };
    let Some(span) = token(raw, 0, end)? else {
        return Ok(None);
    };
    let op = match span.symbol {
        "+" => UnaryNumericOp::Plus,
        "-" => UnaryNumericOp::Minus,
        "@" => UnaryNumericOp::Absolute,
        "~" => UnaryNumericOp::Complement,
        "|/" => UnaryNumericOp::SquareRoot,
        "||/" => UnaryNumericOp::CubeRoot,
        _ => return Err(unsupported_signature()),
    };
    Ok(Some((&raw[..span.end], op)))
}

pub(super) fn binary(span: Span<'_>) -> Result<BinaryOp, SqlError> {
    Ok(match span.symbol {
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Sub,
        "*" => BinaryOp::Mul,
        "/" => BinaryOp::Div,
        "%" => BinaryOp::Modulo,
        "^" => BinaryOp::Power,
        "||" => BinaryOp::Concat,
        "=" => BinaryOp::Eq,
        "<>" | "!=" => BinaryOp::NotEq,
        "<" => BinaryOp::Lt,
        "<=" => BinaryOp::Lte,
        ">" => BinaryOp::Gt,
        ">=" => BinaryOp::Gte,
        "&" => BinaryOp::Bitwise {
            operation: IntegerOperation::And,
        },
        "|" => BinaryOp::Bitwise {
            operation: IntegerOperation::Or,
        },
        "#" => BinaryOp::Bitwise {
            operation: IntegerOperation::Xor,
        },
        "<<" => BinaryOp::Bitwise {
            operation: IntegerOperation::Shl,
        },
        ">>" => BinaryOp::Bitwise {
            operation: IntegerOperation::Shr,
        },
        _ => return Err(unsupported_signature()),
    })
}

fn unsupported_signature() -> SqlError {
    SqlError::unsupported("qualified operator signature remains pending".into())
}

#[cfg(test)]
#[path = "qualified_operator_tests.rs"]
mod tests;
