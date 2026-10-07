use super::clauses::{split_top_level, strip_parentheses};
use super::identifiers::{normalize_identifier, parse_quoted_identifier_chain};
use super::schema::{parse_data_type, starts_with_keyword};
use super::{
    parse_statement, BinaryOp, Expr, FunctionCall, NullsOrder, OrderExpr, QueryStatement,
    SortDirection, SqlError,
};

#[path = "expr_case.rs"]
mod case;

pub(super) fn take_int(input: &str) -> Result<Option<i64>, ParserError> {
    let trimmed = super::lexical::trim_separators(input);
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
    let mut cursor = 0;
    while let Some((position, end)) = super::clauses::find_top_level_keyword_span(s, cursor, ",") {
        out.push(&s[cursor..position]);
        cursor = end;
    }
    out.push(&s[cursor..]);
    out
}

pub(super) fn parse_function(raw: &str) -> Result<Option<FunctionCall>, SqlError> {
    let Some((open, _)) = super::clauses::find_top_level_keyword_span(raw, 0, "(") else {
        return Ok(None);
    };
    let Some(close) = raw.rfind(')') else {
        return Ok(None);
    };
    if close < open {
        return Ok(None);
    }
    let name = super::lexical::trim_separators(&raw[..open]).to_string();
    if name.is_empty() {
        return Ok(None);
    }
    let args_raw = &raw[(open + 1)..close];
    let args = if args_raw.trim().is_empty() {
        Vec::new()
    } else {
        split_csv(args_raw)
            .into_iter()
            .map(parse_function_argument)
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(Some(FunctionCall { name, args }))
}

/// Parses one function argument. An argument that is not a single token,
/// such as `NOT EXISTS (...)` or a comparison, is parsed as a full boolean
/// expression.
fn parse_function_argument(raw: &str) -> Result<Expr, SqlError> {
    parse_expr_token(raw).or_else(|error| parse_expression(raw).map_err(|_| error))
}

pub(crate) fn parse_expression(raw: &str) -> Result<Expr, SqlError> {
    parse_or_expression(super::lexical::trim_separators(raw))
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
    if let Some((left, right)) = split_top_level_conjunction(raw) {
        return Ok(Expr::Binary {
            left: Box::new(parse_and_expression(left)?),
            right: Box::new(parse_and_expression(right)?),
            op: BinaryOp::And,
        });
    }

    parse_not_expression(raw)
}

pub(super) fn parse_not_expression(raw: &str) -> Result<Expr, SqlError> {
    let raw = super::lexical::trim_separators(raw);
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
    let raw = super::lexical::trim_separators(raw);

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
    if split_top_level(raw, " like ")
        .is_some_and(|(_, pattern)| split_top_level(pattern, " escape ").is_some())
    {
        return Err(SqlError::unsupported(
            "LIKE ... ESCAPE is not supported".into(),
        ));
    }
    for (op, parsed) in [
        (" <=> ", BinaryOp::PgvectorCosine),
        (" <-> ", BinaryOp::PgvectorL2),
        (" <#> ", BinaryOp::PgvectorDot),
        ("<=", BinaryOp::Lte),
        (">=", BinaryOp::Gte),
        ("<>", BinaryOp::NotEq),
        ("!=", BinaryOp::NotEq),
        (" like ", BinaryOp::Like),
        ("=", BinaryOp::Eq),
        ("<", BinaryOp::Lt),
        (">", BinaryOp::Gt),
    ] {
        let split = if op.starts_with(' ') {
            split_top_level(raw, op)
        } else {
            super::operators::split_operator(raw, op, false)
        };
        if let Some((left, right)) = split {
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

    parse_arithmetic_expression(raw)
}

fn parse_arithmetic_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right, op)) =
        split_arithmetic_operator(raw, &[("+", BinaryOp::Add), ("-", BinaryOp::Sub)])
    {
        return Ok(Expr::Binary {
            left: Box::new(parse_arithmetic_expression(left)?),
            right: Box::new(parse_arithmetic_expression(right)?),
            op,
        });
    }
    if let Some((left, right, op)) =
        split_arithmetic_operator(raw, &[("*", BinaryOp::Mul), ("/", BinaryOp::Div)])
    {
        return Ok(Expr::Binary {
            left: Box::new(parse_arithmetic_expression(left)?),
            right: Box::new(parse_arithmetic_expression(right)?),
            op,
        });
    }
    parse_expr_token(raw)
}

fn split_arithmetic_operator<'a>(
    raw: &'a str,
    operators: &[(&str, BinaryOp)],
) -> Option<(&'a str, &'a str, BinaryOp)> {
    operators
        .iter()
        .filter_map(|(operator, parsed)| {
            super::operators::split_operator(raw, operator, true)
                .map(|(left, right)| (left, right, parsed.clone()))
        })
        .max_by_key(|(left, _, _)| left.len())
}

/// Splits `raw` at its first top-level `AND` conjunction, skipping each `AND`
/// that closes a preceding `[NOT] BETWEEN low AND high` predicate, so BETWEEN
/// binds tighter than AND as in PostgreSQL.
fn split_top_level_conjunction(raw: &str) -> Option<(&str, &str)> {
    const AND: &str = " and ";
    const BETWEEN: &str = " between ";

    let mut offset = 0;
    let mut open_betweens = 0usize;
    loop {
        let rest = &raw[offset..];
        let and_at = split_top_level(rest, AND).map(|(left, _)| left.len())?;
        let between_at = split_top_level(rest, BETWEEN).map(|(left, _)| left.len());
        if between_at.is_some_and(|between_at| between_at < and_at) {
            open_betweens += 1;
            offset = raw.len() - split_top_level(rest, BETWEEN)?.1.len();
        } else if open_betweens > 0 {
            open_betweens -= 1;
            offset = raw.len() - split_top_level(rest, AND)?.1.len();
        } else {
            let split_at = offset + and_at;
            return Some((&raw[..split_at], split_top_level(rest, AND)?.1));
        }
    }
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
        let token = super::lexical::trim_separators(token);
        let (token, nulls) =
            if let Some(token) = super::lexical::strip_keyword_suffix(token, " nulls first") {
                (token, Some(NullsOrder::First))
            } else if let Some(token) = super::lexical::strip_keyword_suffix(token, " nulls last") {
                (token, Some(NullsOrder::Last))
            } else {
                (token, None)
            };
        let (expr, direction) =
            if let Some(token) = super::lexical::strip_keyword_suffix(token, " desc") {
                (token, SortDirection::Desc)
            } else if let Some(token) = super::lexical::strip_keyword_suffix(token, " asc") {
                (token, SortDirection::Asc)
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
    let raw = super::lexical::trim_separators(raw);
    if raw.is_empty() {
        return Err(SqlError::new("invalid expression token".into()));
    }
    if split_top_level(raw, "||").is_some() {
        return Err(SqlError::unsupported(
            "operator || is not supported; use concat()".into(),
        ));
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
    if starts_with_keyword(raw, "case") {
        return case::parse_case(raw);
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
    if let Some(column) = parse_quoted_identifier_chain(raw)? {
        return Ok(Expr::Column(column.lookup_key()));
    }
    if let Some(value) = parse_single_string_literal(raw) {
        return Ok(Expr::StringLiteral(value));
    }
    // Integer tokens keep integer typing (PostgreSQL types them int4/int8);
    // only tokens with a decimal point or exponent become float literals.
    if is_integer_literal(raw) {
        let value = raw
            .parse::<i64>()
            .map_err(|_| SqlError::new(format!("numeric literal out of range '{raw}'")))?;
        return Ok(Expr::IntegerLiteral(value));
    }
    if let Ok(value) = raw.parse::<f64>() {
        if !value.is_finite() {
            return Err(SqlError::new(format!(
                "numeric literal out of range '{raw}'"
            )));
        }
        return Ok(Expr::NumberLiteral(value));
    }
    if super::operators::numeric_exponent_prefix(raw) {
        return Err(SqlError::new(format!("invalid numeric literal '{raw}'")));
    }
    if let Some(exists) = parse_exists_expression(raw)? {
        return Ok(exists);
    }
    if let Some(cast) = parse_cast_expression(raw)? {
        return Ok(cast);
    }
    if raw != "*" && super::operators::contains_operator(raw) {
        return Err(SqlError::new(format!("invalid expression token '{raw}'")));
    }
    if let Some(func) = parse_function(raw)? {
        return Ok(Expr::Function(func));
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError::new(format!("invalid expression token '{raw}'")));
    }

    let path = crate::sql::ColumnIdentifierPath::parse(raw).map_err(SqlError::new)?;
    Ok(Expr::Column(path.lookup_key()))
}

/// Parses `raw` as exactly one single-quoted literal: the opening quote's
/// match must be the final character, with `''` read as an escaped quote.
fn parse_single_string_literal(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('\'')?;
    let mut value = String::with_capacity(inner.len());
    let mut chars = inner.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '\'' {
            value.push(ch);
        } else if chars.next_if(|&(_, next)| next == '\'').is_some() {
            value.push('\'');
        } else {
            return (idx + 1 == inner.len()).then_some(value);
        }
    }
    None
}

fn is_integer_literal(raw: &str) -> bool {
    let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn parse_exists_expression(raw: &str) -> Result<Option<Expr>, SqlError> {
    let trimmed = raw.trim();
    if !starts_with_keyword(trimmed, "exists") {
        return Ok(None);
    }
    let inner = strip_parentheses(super::lexical::trim_separators(&trimmed[6..]))
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
    let inner = strip_parentheses(super::lexical::trim_separators(&trimmed[4..]))
        .ok_or_else(|| SqlError::new("CAST requires parenthesized expression".into()))?;
    let (expr_raw, type_raw) = split_top_level(inner, " as ")
        .ok_or_else(|| SqlError::new("CAST requires AS type clause".into()))?;
    let data_type = parse_data_type(super::lexical::trim_separators(type_raw))?;

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
        let alias = super::lexical::trim_separators(right);
        let alias = normalize_identifier(alias).unwrap_or_else(|_| alias.to_string());
        return (left.trim(), Some(alias));
    }
    (token, None)
}
