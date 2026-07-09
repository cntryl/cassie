use super::clauses::{split_top_level, strip_parentheses};
use super::schema::{parse_data_type, starts_with_keyword};
use super::{
    parse_statement, BinaryOp, Expr, FunctionCall, NullsOrder, OrderExpr, QueryStatement,
    SortDirection, SqlError,
};

pub(super) fn take_int(input: &str) -> Result<Option<i64>, ParserError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = trimmed
        .parse::<i64>()
        .map_err(|_| ParserError::InvalidClause(trimmed.to_string()))?;

    if parsed < 0 {
        return Err(ParserError::NegativeValue(trimmed.to_string()));
    }

    Ok(Some(parsed))
}

#[derive(Debug)]
pub(super) enum ParserError {
    InvalidClause(String),
    NegativeValue(String),
}

impl std::fmt::Display for ParserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClause(value) => write!(f, "invalid clause value: '{value}'"),
            Self::NegativeValue(value) => {
                write!(f, "negative clause value not supported: '{value}'")
            }
        }
    }
}

pub(super) fn split_csv(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth = depth.saturating_sub(1);
            }
            '[' if !in_single && !in_double => bracket_depth += 1,
            ']' if !in_single && !in_double => {
                bracket_depth = bracket_depth.saturating_sub(1);
            }
            ',' if !in_single && !in_double && depth == 0 && bracket_depth == 0 => {
                out.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

pub(super) fn split_csv_quoted_by_space(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

pub(super) fn parse_function(raw: &str) -> Result<Option<FunctionCall>, SqlError> {
    let Some(open) = raw.find('(') else {
        return Ok(None);
    };
    let Some(close) = raw.rfind(')') else {
        return Ok(None);
    };
    if close < open {
        return Ok(None);
    }
    let name = raw[..open].trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }
    let args_raw = &raw[(open + 1)..close];
    let args = if args_raw.trim().is_empty() {
        Vec::new()
    } else {
        split_csv(args_raw)
            .into_iter()
            .map(parse_expr_token)
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(Some(FunctionCall { name, args }))
}

pub(crate) fn parse_expression(raw: &str) -> Result<Expr, SqlError> {
    parse_or_expression(raw)
}

pub(super) fn parse_or_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right)) = split_top_level(raw, " or ") {
        return Ok(Expr::Binary {
            left: Box::new(parse_or_expression(left)?),
            right: Box::new(parse_or_expression(right)?),
            op: BinaryOp::Or,
        });
    }

    parse_and_expression(raw)
}

pub(super) fn parse_and_expression(raw: &str) -> Result<Expr, SqlError> {
    if contains_top_level_between(raw) {
        return parse_not_expression(raw);
    }

    if let Some((left, right)) = split_top_level(raw, " and ") {
        return Ok(Expr::Binary {
            left: Box::new(parse_and_expression(left)?),
            right: Box::new(parse_and_expression(right)?),
            op: BinaryOp::And,
        });
    }

    parse_not_expression(raw)
}

pub(super) fn parse_not_expression(raw: &str) -> Result<Expr, SqlError> {
    let raw = raw.trim();
    if starts_with_keyword(raw, "not") {
        let rest = raw["not".len()..].trim();
        if rest.is_empty() {
            return Err(SqlError::new("NOT requires an expression".into()));
        }
        return Ok(Expr::Not {
            expr: Box::new(parse_not_expression(rest)?),
        });
    }

    parse_comparison_expression(raw)
}

pub(super) fn parse_comparison_expression(raw: &str) -> Result<Expr, SqlError> {
    let raw = raw.trim();

    if raw.starts_with('(') {
        let inner = strip_parentheses(raw);
        if let Some(inner) = inner {
            return parse_expression(inner);
        }
    }

    if let Some((left, right)) = split_top_level(raw, " is not null") {
        if right.trim().is_empty() {
            return Ok(Expr::IsNull {
                expr: Box::new(parse_comparison_expression(left)?),
                negated: true,
            });
        }
    }
    if let Some((left, right)) = split_top_level(raw, " is null") {
        if right.trim().is_empty() {
            return Ok(Expr::IsNull {
                expr: Box::new(parse_comparison_expression(left)?),
                negated: false,
            });
        }
    }
    if let Some((left, right)) = split_top_level(raw, " not in ") {
        return parse_in_list_expression(left, right, true);
    }
    if let Some((left, right)) = split_top_level(raw, " in ") {
        return parse_in_list_expression(left, right, false);
    }
    if let Some((left, right)) = split_top_level(raw, " not between ") {
        return parse_between_expression(left, right, true);
    }
    if let Some((left, right)) = split_top_level(raw, " between ") {
        return parse_between_expression(left, right, false);
    }
    for (op, parsed) in [
        (" <=> ", BinaryOp::PgvectorCosine),
        (" <-> ", BinaryOp::PgvectorL2),
        (" <#> ", BinaryOp::PgvectorDot),
        (" <= ", BinaryOp::Lte),
        (" >= ", BinaryOp::Gte),
        (" <> ", BinaryOp::NotEq),
        (" != ", BinaryOp::NotEq),
        (" like ", BinaryOp::Like),
        (" = ", BinaryOp::Eq),
        (" < ", BinaryOp::Lt),
        (" > ", BinaryOp::Gt),
    ] {
        if let Some((left, right)) = split_top_level(raw, op) {
            return Ok(Expr::Binary {
                left: Box::new(parse_comparison_expression(left)?),
                right: Box::new(parse_comparison_expression(right)?),
                op: parsed,
            });
        }
    }

    if let Some((left, right)) = split_top_level(raw, "::") {
        let data_type = parse_data_type(right.trim())?;
        return Ok(Expr::Cast {
            expr: Box::new(parse_comparison_expression(left)?),
            data_type,
        });
    }

    parse_expr_token(raw)
}

pub(super) fn contains_top_level_between(raw: &str) -> bool {
    split_top_level(raw, " between ").is_some() || split_top_level(raw, " not between ").is_some()
}

pub(super) fn parse_between_expression(
    left: &str,
    right: &str,
    negated: bool,
) -> Result<Expr, SqlError> {
    let (low, high) = split_top_level(right, " and ")
        .ok_or_else(|| SqlError::new("BETWEEN predicate requires AND upper bound".into()))?;
    if high.trim().is_empty() {
        return Err(SqlError::new(
            "BETWEEN predicate requires an upper bound".to_string(),
        ));
    }

    Ok(Expr::Between {
        expr: Box::new(parse_comparison_expression(left)?),
        low: Box::new(parse_comparison_expression(low)?),
        high: Box::new(parse_comparison_expression(high)?),
        negated,
    })
}

pub(super) fn parse_in_list_expression(
    left: &str,
    right: &str,
    negated: bool,
) -> Result<Expr, SqlError> {
    let values_raw = strip_parentheses(right.trim())
        .ok_or_else(|| SqlError::new("IN predicate requires a parenthesized value list".into()))?;
    if values_raw.trim().is_empty() {
        return Err(SqlError::new(
            "IN predicate requires at least one value".into(),
        ));
    }
    let values = split_csv(values_raw)
        .into_iter()
        .map(parse_expression)
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(SqlError::new(
            "IN predicate requires at least one value".into(),
        ));
    }

    Ok(Expr::InList {
        expr: Box::new(parse_comparison_expression(left)?),
        values,
        negated,
    })
}

pub(super) fn parse_order_by(raw: &str) -> Result<Vec<OrderExpr>, SqlError> {
    let mut items = Vec::new();
    for token in split_csv(raw) {
        let token = token.trim();
        let lower = token.to_lowercase();
        let (token, nulls) = if lower.ends_with(" nulls first") {
            (
                token[..token.len() - " nulls first".len()].trim(),
                Some(NullsOrder::First),
            )
        } else if lower.ends_with(" nulls last") {
            (
                token[..token.len() - " nulls last".len()].trim(),
                Some(NullsOrder::Last),
            )
        } else {
            (token, None)
        };
        let lower = token.to_lowercase();
        let (expr, direction) = if lower.ends_with(" desc") {
            (&token[..token.len() - 5], SortDirection::Desc)
        } else if lower.ends_with(" asc") {
            (&token[..token.len() - 4], SortDirection::Asc)
        } else {
            (token, SortDirection::Asc)
        };
        items.push(OrderExpr {
            expr: parse_expression(expr)?,
            direction,
            nulls,
        });
    }
    Ok(items)
}

pub(super) fn parse_expr_token(raw: &str) -> Result<Expr, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("invalid expression token".into()));
    }

    if raw.starts_with('$') {
        let value = raw.trim_start_matches('$');
        if value.is_empty() {
            return Err(SqlError::new("invalid parameter index".into()));
        }

        let idx = value
            .parse::<usize>()
            .map_err(|_| SqlError::new(format!("invalid parameter index '{raw}'")))?;
        if idx == 0 {
            return Err(SqlError::new(format!("invalid parameter index '{raw}'")));
        }
        return Ok(Expr::Param(idx - 1));
    }
    if raw.eq_ignore_ascii_case("null") {
        return Ok(Expr::Null);
    }
    if raw.eq_ignore_ascii_case("true") {
        return Ok(Expr::BoolLiteral(true));
    }
    if raw.eq_ignore_ascii_case("false") {
        return Ok(Expr::BoolLiteral(false));
    }
    if raw.starts_with('"') && raw.ends_with('"') {
        return Ok(Expr::StringLiteral(raw.trim_matches('"').to_string()));
    }
    if raw.starts_with('\'') && raw.ends_with('\'') {
        return Ok(Expr::StringLiteral(raw.trim_matches('\'').to_string()));
    }
    if let Ok(v) = raw.parse::<f64>() {
        return Ok(Expr::NumberLiteral(v));
    }
    if let Some(exists) = parse_exists_expression(raw)? {
        return Ok(exists);
    }
    if let Some(cast) = parse_cast_expression(raw)? {
        return Ok(cast);
    }
    if let Some(func) = parse_function(raw)? {
        return Ok(Expr::Function(func));
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError::new(format!("invalid expression token '{raw}'")));
    }

    Ok(Expr::Column(raw.to_string()))
}

pub(super) fn parse_exists_expression(raw: &str) -> Result<Option<Expr>, SqlError> {
    let trimmed = raw.trim();
    if !starts_with_keyword(trimmed, "exists") {
        return Ok(None);
    }
    let inner = strip_parentheses(trimmed[6..].trim())
        .ok_or_else(|| SqlError::new("EXISTS requires a parenthesized subquery".into()))?;
    let parsed = parse_statement(inner)?;
    if !matches!(parsed.statement, QueryStatement::Select(_)) {
        return Err(SqlError::new("EXISTS requires a SELECT subquery".into()));
    }

    Ok(Some(Expr::Exists(Box::new(parsed))))
}

pub(super) fn parse_cast_expression(raw: &str) -> Result<Option<Expr>, SqlError> {
    let trimmed = raw.trim();
    if !starts_with_keyword(trimmed, "cast") {
        return Ok(None);
    }
    let inner = strip_parentheses(trimmed[4..].trim())
        .ok_or_else(|| SqlError::new("CAST requires parenthesized expression".into()))?;
    let (expr_raw, type_raw) = split_top_level(inner, " as ")
        .ok_or_else(|| SqlError::new("CAST requires AS type clause".into()))?;
    let data_type = parse_data_type(type_raw.trim())?;

    Ok(Some(Expr::Cast {
        expr: Box::new(parse_expression(expr_raw)?),
        data_type,
    }))
}

pub(super) fn parse_alias(raw: &str) -> (&str, Option<String>) {
    let token = raw.trim();
    if let Some((left, right)) = split_top_level(token, " as ") {
        if right.trim().is_empty() {
            return (token, None);
        }
        return (left.trim(), Some(right.trim().to_string()));
    }
    (token, None)
}
