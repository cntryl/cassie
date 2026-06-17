use crate::sql::ast::{
    BinaryOp, Expr, FunctionCall, OrderExpr, ParsedStatement, QueryStatement, SelectItem,
    SelectStatement, SortDirection,
};

#[derive(Debug)]
pub struct SqlError(pub String);

pub fn parse_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if !trimmed.to_lowercase().starts_with("select") {
        return Err(SqlError(
            "only SELECT statements are supported in this stage".into(),
        ));
    }

    let lower = trimmed.to_lowercase();
    let from_pos = lower
        .find(" from ")
        .ok_or_else(|| SqlError("missing FROM clause".into()))?;

    let select_part = &trimmed[6..from_pos].trim();
    let rest = trimmed[(from_pos + 6)..].trim();

    let lower_rest = rest.to_lowercase();
    let mut clauses = Vec::new();
    for (token, kind) in [
        (" where ", Clause::Where),
        (" order by ", Clause::Order),
        (" limit ", Clause::Limit),
        (" offset ", Clause::Offset),
    ] {
        if let Some(position) = lower_rest.find(token) {
            clauses.push((position, token, kind));
        }
    }
    clauses.sort_unstable_by_key(|entry| entry.0);

    let first_clause = clauses
        .first()
        .map(|(pos, _, _)| *pos)
        .unwrap_or(rest.len());
    let from_source = rest[..first_clause].trim();

    let mut where_clause: Option<String> = None;
    let mut order_clause: Option<String> = None;
    let mut limit_clause: Option<i64> = None;
    let mut offset_clause: Option<i64> = None;

    for (idx, (position, token, kind)) in clauses.iter().enumerate() {
        let start = *position + token.len();
        let end = clauses
            .get(idx + 1)
            .map(|(next_pos, _, _)| *next_pos)
            .unwrap_or_else(|| rest.len());
        let value = rest[start..end].trim();
        if value.is_empty() {
            continue;
        }
        match kind {
            Clause::Where => where_clause = Some(value.to_string()),
            Clause::Order => order_clause = Some(value.to_string()),
            Clause::Limit => {
                limit_clause = take_int(value).map_err(|error| SqlError(error.to_string()))?
            }
            Clause::Offset => {
                offset_clause = take_int(value).map_err(|error| SqlError(error.to_string()))?
            }
        }
    }

    let projection_tokens: Vec<&str> = split_csv(select_part);
    let mut projection = Vec::with_capacity(projection_tokens.len());
    for token in projection_tokens {
        let token = token.trim();
        if token == "*" {
            projection.push(SelectItem::Wildcard);
        } else if let Some(call) = parse_function(token) {
            let (_expr, alias) = parse_alias(token);
            let function = call;
            if let Some(raw) = alias {
                projection.push(SelectItem::Function {
                    function,
                    alias: Some(raw),
                });
            } else {
                projection.push(SelectItem::Function {
                    function,
                    alias: None,
                });
            }
        } else {
            let (expr, alias) = parse_alias(token);
            projection.push(SelectItem::Column {
                name: expr.to_string(),
                alias,
            });
        }
    }

    let tokens: Vec<&str> = split_csv_quoted_by_space(from_source);
    if tokens.is_empty() {
        return Err(SqlError("missing collection in FROM".into()));
    }
    let collection = tokens[0].to_string();

    let filter = where_clause.as_deref().map(parse_expression).transpose()?;
    let order = order_clause.as_deref().map(parse_order_by).transpose()?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Select(SelectStatement {
            collection,
            projection,
            filter,
            order: order.unwrap_or_default(),
            limit: limit_clause,
            offset: offset_clause,
        }),
    })
}

fn take_int(input: &str) -> Result<Option<i64>, ParserError> {
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
enum ParserError {
    InvalidClause(String),
    NegativeValue(String),
}

impl std::fmt::Display for ParserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidClause(value) => write!(f, "invalid clause value: '{value}'"),
            Self::NegativeValue(value) => write!(f, "negative clause value not supported: '{value}'"),
        }
    }
}

fn split_csv(s: &str) -> Vec<&str> {
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
                continue;
            }
            '"' if !in_single => {
                in_double = !in_double;
                continue;
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
                continue;
            }
        _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn split_csv_quoted_by_space(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

fn parse_function(raw: &str) -> Option<FunctionCall> {
    let open = raw.find('(')?;
    let close = raw.rfind(')')?;
    if close < open {
        return None;
    }
    let name = raw[..open].trim().to_string();
    let args_raw = &raw[(open + 1)..close];
    let args = split_csv(args_raw)
        .into_iter()
        .map(parse_expr_token)
        .collect::<Vec<_>>();
    Some(FunctionCall { name, args })
}

fn parse_expression(raw: &str) -> Result<Expr, SqlError> {
    parse_or_expression(raw)
}

fn parse_or_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right)) = split_top_level(raw, " or ") {
        return Ok(Expr::Binary {
            left: Box::new(parse_or_expression(left)?),
            right: Box::new(parse_or_expression(right)?),
            op: BinaryOp::Or,
        });
    }

    parse_and_expression(raw)
}

fn parse_and_expression(raw: &str) -> Result<Expr, SqlError> {
    if let Some((left, right)) = split_top_level(raw, " and ") {
        return Ok(Expr::Binary {
            left: Box::new(parse_and_expression(left)?),
            right: Box::new(parse_and_expression(right)?),
            op: BinaryOp::And,
        });
    }

    parse_comparison_expression(raw)
}

fn parse_comparison_expression(raw: &str) -> Result<Expr, SqlError> {
    let raw = raw.trim();

    if raw.starts_with('(') {
        let inner = strip_parentheses(raw);
        if let Some(inner) = inner {
            return parse_expression(inner);
        }
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

    Ok(parse_expr_token(raw))
}

fn parse_order_by(raw: &str) -> Result<Vec<OrderExpr>, SqlError> {
    let mut items = Vec::new();
    for token in split_csv(raw) {
        let token = token.trim();
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
        });
    }
    Ok(items)
}

fn parse_expr_token(raw: &str) -> Expr {
    let raw = raw.trim();
    if raw.starts_with('$') {
        let idx = raw
            .trim_start_matches('$')
            .parse::<usize>()
            .unwrap_or(1)
            .saturating_sub(1);
        return Expr::Param(idx);
    }
    if raw.eq_ignore_ascii_case("null") {
        return Expr::Null;
    }
    if raw.eq_ignore_ascii_case("true") {
        return Expr::BoolLiteral(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return Expr::BoolLiteral(false);
    }
    if raw.starts_with('"') && raw.ends_with('"') {
        return Expr::StringLiteral(raw.trim_matches('"').to_string());
    }
    if raw.starts_with('\'') && raw.ends_with('\'') {
        return Expr::StringLiteral(raw.trim_matches('\'').to_string());
    }
    if let Ok(v) = raw.parse::<f64>() {
        return Expr::NumberLiteral(v);
    }
    if let Some(func) = parse_function(raw) {
        return Expr::Function(func);
    }
    Expr::Column(raw.to_string())
}

fn parse_alias(raw: &str) -> (&str, Option<String>) {
    let token = raw.trim();
    let lower = token.to_lowercase();
    if let Some(at) = lower.rfind(" as ") {
        let left = &token[..at].trim();
        let alias = token[(at + 4)..].trim().to_string();
        return (left, Some(alias));
    }
    (token, None)
}

#[derive(Debug, Clone, Copy)]
enum Clause {
    Where,
    Order,
    Limit,
    Offset,
}

fn split_top_level<'a>(input: &'a str, keyword: &'a str) -> Option<(&'a str, &'a str)> {
    let lower = input.to_lowercase();
    let chars = lower.char_indices().collect::<Vec<_>>();
    let token = keyword.as_bytes();
    let mut depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    for &(idx, ch) in &chars {
        match ch {
            '\'' => {
                if !in_double {
                    in_single = !in_single;
                }
            }
            '"' => {
                if !in_single {
                    in_double = !in_double;
                }
            }
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth = depth.saturating_sub(1),
            '[' if !in_single && !in_double => bracket_depth += 1,
            ']' if !in_single && !in_double => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }

        if depth == 0
            && bracket_depth == 0
            && !in_single
            && !in_double
            && idx + token.len() <= input.len()
        {
            let slice = &lower[idx..idx + token.len()];
            if slice.as_bytes() == token {
                return Some((&input[..idx], &input[idx + token.len()..]));
            }
        }
    }

    None
}

fn strip_parentheses(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
        return None;
    }

    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in trimmed.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth -= 1,
            _ => {}
        }

        if depth == 0 && i != trimmed.len().saturating_sub(1) {
            return None;
        }
    }

    Some(trimmed[1..trimmed.len() - 1].trim())
}
