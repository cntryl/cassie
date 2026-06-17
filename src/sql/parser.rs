use crate::sql::ast::{
    BinaryOp, Expr, FunctionCall, OrderExpr, ParsedStatement, QueryStatement, SelectItem,
    SelectStatement, SortDirection,
};
use std::collections::HashSet;

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

    let select_clause = &trimmed[..from_pos].trim();
    if select_clause.is_empty() || !select_clause.to_lowercase().starts_with("select") {
        return Err(SqlError("missing projection in SELECT statement".into()));
    }

    let select_part = &trimmed[6..from_pos].trim();
    let rest = trimmed[(from_pos + 6)..].trim();

    let clauses = parse_clauses(rest)?;

    let first_clause = clauses
        .first()
        .map(|clause| clause.position)
        .unwrap_or_else(|| rest.len());
    let from_source = rest[..first_clause].trim();

    if from_source.is_empty() {
        return Err(SqlError("missing collection in FROM".into()));
    }

    let mut where_clause: Option<String> = None;
    let mut order_clause: Option<String> = None;
    let mut limit_clause: Option<i64> = None;
    let mut offset_clause: Option<i64> = None;

    let mut seen = HashSet::new();
    for (idx, clause) in clauses.iter().enumerate() {
        let next_pos = clauses
            .get(idx + 1)
            .map(|clause| clause.position)
            .unwrap_or_else(|| rest.len());

        let token_text = match clause.token {
            ClauseToken::Recognized(clause_kind) => clause_kind.token(),
            ClauseToken::Unsupported(kind) => kind,
        };
        let start = clause.position + token_text.len();
        if start > rest.len() || next_pos > rest.len() || start > next_pos {
            return Err(SqlError(format!(
                "unsupported or malformed clause placement: {}",
                clause.text()
            )));
        }

        let raw_value = rest[start..next_pos].trim();
        if raw_value.is_empty() {
            return Err(SqlError(format!(
                "missing value for clause '{}'",
                clause.text()
            )));
        }

        match clause.token {
            ClauseToken::Unsupported(kind) => {
                return Err(SqlError(format!("unsupported clause '{}'", kind)));
            }
            ClauseToken::Recognized(kind) => match kind {
                Clause::Where => {
                    if !seen.insert("where") {
                        return Err(SqlError("duplicate WHERE clause".into()));
                    }
                    where_clause = Some(raw_value.to_string());
                }
                Clause::Order => {
                    if !seen.insert("order by") {
                        return Err(SqlError("duplicate ORDER BY clause".into()));
                    }
                    order_clause = Some(raw_value.to_string());
                }
                Clause::Limit => {
                    if !seen.insert("limit") {
                        return Err(SqlError("duplicate LIMIT clause".into()));
                    }
                    limit_clause =
                        take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
                }
                Clause::Offset => {
                    if !seen.insert("offset") {
                        return Err(SqlError("duplicate OFFSET clause".into()));
                    }
                    offset_clause =
                        take_int(raw_value).map_err(|error| SqlError(error.to_string()))?;
                }
            },
        }
    }

    let from_tokens: Vec<&str> = split_csv_quoted_by_space(from_source);
    if from_tokens.is_empty() {
        return Err(SqlError("missing collection in FROM".into()));
    }

    if from_tokens.len() != 1 {
        return Err(SqlError("unsupported FROM syntax".into()));
    }

    let collection = from_tokens[0].to_string();

    let projection_tokens: Vec<&str> = split_csv(select_part);
    let mut projection = Vec::with_capacity(projection_tokens.len());
    for token in projection_tokens {
        let token = token.trim();
        if token == "*" {
            projection.push(SelectItem::Wildcard);
        } else if let Some(call) = parse_function(token)? {
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
            Self::NegativeValue(value) => {
                write!(f, "negative clause value not supported: '{value}'")
            }
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

fn parse_function(raw: &str) -> Result<Option<FunctionCall>, SqlError> {
    let open = match raw.find('(') {
        Some(value) => value,
        None => return Ok(None),
    };
    let close = match raw.rfind(')') {
        Some(value) => value,
        None => return Ok(None),
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

    parse_expr_token(raw)
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

fn parse_expr_token(raw: &str) -> Result<Expr, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError("invalid expression token".into()));
    }

    if raw.starts_with('$') {
        let value = raw.trim_start_matches('$');
        if value.is_empty() {
            return Err(SqlError("invalid parameter index".into()));
        }

        let idx = value
            .parse::<usize>()
            .map_err(|_| SqlError(format!("invalid parameter index '{raw}'")))?;
        if idx == 0 {
            return Err(SqlError(format!("invalid parameter index '{raw}'")));
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
    if let Some(func) = parse_function(raw)? {
        return Ok(Expr::Function(func));
    }

    if raw.chars().any(char::is_whitespace) {
        return Err(SqlError(format!("invalid expression token '{raw}'")));
    }

    Ok(Expr::Column(raw.to_string()))
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

impl Clause {
    fn token(self) -> &'static str {
        match self {
            Self::Where => "where",
            Self::Order => "order by",
            Self::Limit => "limit",
            Self::Offset => "offset",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Where => "WHERE",
            Self::Order => "ORDER BY",
            Self::Limit => "LIMIT",
            Self::Offset => "OFFSET",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ClauseToken {
    Recognized(Clause),
    Unsupported(&'static str),
}

#[derive(Debug)]
struct ClauseMatch {
    position: usize,
    token: ClauseToken,
}

impl ClauseMatch {
    fn text(&self) -> &'static str {
        match self.token {
            ClauseToken::Recognized(kind) => kind.name(),
            ClauseToken::Unsupported(text) => text,
        }
    }
}

fn parse_clauses(rest: &str) -> Result<Vec<ClauseMatch>, SqlError> {
    let mut matches = Vec::new();

    for token in [
        ("where", ClauseToken::Recognized(Clause::Where)),
        ("order by", ClauseToken::Recognized(Clause::Order)),
        ("limit", ClauseToken::Recognized(Clause::Limit)),
        ("offset", ClauseToken::Recognized(Clause::Offset)),
        ("group by", ClauseToken::Unsupported("GROUP BY")),
        ("having", ClauseToken::Unsupported("HAVING")),
        ("union", ClauseToken::Unsupported("UNION")),
        ("intersect", ClauseToken::Unsupported("INTERSECT")),
        ("except", ClauseToken::Unsupported("EXCEPT")),
        ("join", ClauseToken::Unsupported("JOIN")),
    ] {
        let mut cursor = 0;
        while let Some(position) = find_top_level_clause(rest, cursor, token.0) {
            matches.push(ClauseMatch {
                position,
                token: token.1,
            });
            cursor = position + 1;
        }
    }

    matches.sort_by_key(|entry| entry.position);

    for window in matches.windows(2) {
        if window[0].position == window[1].position {
            return Err(SqlError(format!(
                "ambiguous clause token '{}' at position {}",
                window[0].text(),
                window[0].position,
            )));
        }
    }

    let mut ordered = Vec::new();
    for clause in matches {
        if let ClauseToken::Unsupported(kind) = clause.token {
            return Err(SqlError(format!("unsupported clause '{}'", kind)));
        }
        ordered.push(clause);
    }

    Ok(ordered)
}

fn find_top_level_clause(rest: &str, start: usize, token: &str) -> Option<usize> {
    let lower = rest.to_lowercase();
    let token = token.as_bytes();
    let bytes = lower.as_bytes();
    let mut depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in lower.char_indices() {
        if idx < start {
            match ch {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '(' if !in_single && !in_double => depth += 1,
                ')' if !in_single && !in_double => depth = depth.saturating_sub(1),
                '[' if !in_single && !in_double => bracket_depth += 1,
                ']' if !in_single && !in_double => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth = depth.saturating_sub(1),
            '[' if !in_single && !in_double => bracket_depth += 1,
            ']' if !in_single && !in_double => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }

        if depth != 0 || bracket_depth != 0 || in_single || in_double {
            continue;
        }

        if idx + token.len() > bytes.len() {
            continue;
        }

        if &bytes[idx..idx + token.len()] == token
            && is_clause_boundary_before(lower.as_bytes(), idx)
            && is_clause_boundary_after(lower.as_bytes(), idx + token.len())
        {
            return Some(idx);
        }
    }

    None
}

fn is_clause_boundary_before(bytes: &[u8], index: usize) -> bool {
    index == 0 || !is_identifier_byte(*bytes.get(index.saturating_sub(1)).unwrap_or(&b' '))
}

fn is_clause_boundary_after(bytes: &[u8], index: usize) -> bool {
    index >= bytes.len() || !is_identifier_byte(*bytes.get(index).unwrap_or(&b' '))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
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
