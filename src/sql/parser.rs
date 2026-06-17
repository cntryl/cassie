use crate::sql::ast::{
    AlterTableOperation, AlterTableStatement, BinaryOp, CommonTableExpression,
    CreateSchemaStatement, CreateTableStatement, CteQuery, DropTableStatement, Expr,
    FieldDefinition, FunctionCall, OrderExpr, ParsedStatement, QuerySource, QueryStatement,
    SelectItem, SelectStatement, SortDirection,
};
use crate::types::DataType;
use std::collections::HashSet;

#[derive(Debug)]
pub struct SqlError(pub String);

pub fn parse_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_lowercase();
    if lower.starts_with("with ") || lower == "with" {
        parse_with_statement(trimmed)
    } else if lower.starts_with("select ") {
        parse_select_statement(trimmed, Vec::new(), false)
    } else if lower.starts_with("create table ") || lower == "create table" {
        parse_create_table_statement(trimmed)
    } else if lower.starts_with("drop table ") || lower == "drop table" {
        parse_drop_table_statement(trimmed)
    } else if lower.starts_with("alter table ") || lower == "alter table" {
        parse_alter_table_statement(trimmed)
    } else if lower.starts_with("create schema ") || lower == "create schema" {
        parse_create_schema_statement(trimmed)
    } else {
        Err(SqlError("unsupported SQL statement".into()))
    }
}

fn parse_with_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let remainder = sql[4..].trim_start();
    let lower_remainder = remainder.to_lowercase();
    let mut recursive = false;
    let after_recursive = if lower_remainder.starts_with("recursive ") {
        recursive = true;
        remainder[10..].trim_start()
    } else {
        remainder
    };

    let select_pos = find_top_level_keyword(after_recursive, 0, "select")
        .ok_or_else(|| SqlError("missing SELECT after WITH clause".into()))?;

    let cte_sql = after_recursive[..select_pos].trim();
    if cte_sql.is_empty() {
        return Err(SqlError("missing CTE definition in WITH clause".into()));
    }
    if !after_recursive[select_pos..]
        .to_lowercase()
        .starts_with("select ")
    {
        return Err(SqlError(
            "only SELECT statements are supported in this stage".into(),
        ));
    }

    let cte_defs = parse_cte_definitions(cte_sql, recursive)?;
    let main_select = &after_recursive[select_pos..];
    parse_select_statement(main_select, cte_defs, recursive)
}

fn parse_create_table_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[12..].trim();

    let (if_not_exists, rest) = parse_if_not_exists(rest)?;

    let open_paren = rest
        .find('(')
        .ok_or_else(|| SqlError("CREATE TABLE requires a column list".into()))?;
    let close_paren = rest
        .rfind(')')
        .ok_or_else(|| SqlError("CREATE TABLE requires closing ')'".into()))?;
    if close_paren < open_paren {
        return Err(SqlError("invalid CREATE TABLE definition".into()));
    }

    let table = rest[..open_paren].trim();
    let body = rest[(open_paren + 1)..close_paren].trim();
    let trailing = rest[(close_paren + 1)..].trim();
    if !trailing.is_empty() {
        return Err(SqlError(
            "unexpected tokens after CREATE TABLE columns".into(),
        ));
    }
    if table.is_empty() {
        return Err(SqlError("missing table name".into()));
    }

    let mut fields = Vec::new();
    if !body.is_empty() {
        for raw in split_csv(body) {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(SqlError("empty column definition".into()));
            }
            let field = parse_field_definition(raw)?;
            fields.push(field);
        }
    }

    if fields.is_empty() {
        return Err(SqlError("CREATE TABLE requires at least one column".into()));
    }

    let mut seen = HashSet::new();
    for field in &fields {
        let name = field.name.to_ascii_lowercase();
        if !seen.insert(name.clone()) {
            return Err(SqlError(format!("duplicate column name '{name}'")));
        }
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateTable(CreateTableStatement {
            table: table.to_string(),
            fields,
            if_not_exists,
        }),
    })
}

fn parse_drop_table_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[10..].trim();

    let (if_exists, rest) = parse_if_exists(rest)?;
    let table = rest.trim();
    if table.is_empty() {
        return Err(SqlError("missing table name in DROP TABLE".into()));
    }
    if table.split_whitespace().count() != 1 {
        return Err(SqlError(
            "DROP TABLE supports only a single table name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropTable(DropTableStatement {
            table: table.to_string(),
            if_exists,
        }),
    })
}

fn parse_alter_table_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[11..].trim();

    let (if_exists, rest) = parse_if_exists(rest)?;
    if if_exists {
        return Err(SqlError("ALTER TABLE IF EXISTS is not supported".into()));
    }
    let mut table_and_op = rest.splitn(2, char::is_whitespace);
    let table = table_and_op
        .next()
        .ok_or_else(|| SqlError("missing table name in ALTER TABLE".into()))?
        .trim();
    if table.is_empty() {
        return Err(SqlError("missing table name in ALTER TABLE".into()));
    }
    if table.contains(' ') {
        return Err(SqlError("invalid table name in ALTER TABLE".into()));
    }

    let op_clause = table_and_op
        .next()
        .ok_or_else(|| SqlError("missing alter operation".into()))?
        .trim();
    if op_clause.is_empty() {
        return Err(SqlError("missing alter operation".into()));
    }

    let operation = parse_alter_table_operation(op_clause)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterTable(AlterTableStatement {
            table: table.to_string(),
            operation,
        }),
    })
}

fn parse_alter_table_operation(raw: &str) -> Result<AlterTableOperation, SqlError> {
    let lower = raw.to_lowercase();
    if lower.starts_with("add column") {
        let field_def = raw["add column".len()..].trim();
        let definition = parse_field_definition(field_def)?;
        return Ok(AlterTableOperation::AddColumn {
            field: definition.name,
            data_type: definition.data_type,
        });
    }
    if lower.starts_with("drop column") {
        let field = raw["drop column".len()..].trim();
        if field.is_empty() {
            return Err(SqlError("DROP COLUMN requires a column name".into()));
        }
        if field.split_whitespace().count() != 1 {
            return Err(SqlError("DROP COLUMN supports only one column".into()));
        }
        return Ok(AlterTableOperation::DropColumn {
            field: field.to_string(),
        });
    }
    if lower.starts_with("rename to") {
        let table = raw["rename to".len()..].trim();
        if table.is_empty() {
            return Err(SqlError("RENAME TO requires a collection name".into()));
        }
        if table.split_whitespace().count() != 1 {
            return Err(SqlError(
                "RENAME TO supports only one collection name".into(),
            ));
        }
        return Ok(AlterTableOperation::RenameTo {
            table: table.to_string(),
        });
    }

    Err(SqlError("unsupported ALTER TABLE operation".into()))
}

fn parse_create_schema_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[13..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest)?;
    let schema = rest.trim();
    if schema.is_empty() {
        return Err(SqlError("missing schema name".into()));
    }
    if schema.split_whitespace().count() != 1 {
        return Err(SqlError(
            "CREATE SCHEMA supports only one schema name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateSchema(CreateSchemaStatement {
            schema: schema.to_string(),
            if_not_exists,
        }),
    })
}

fn parse_if_not_exists(raw: &str) -> Result<(bool, &str), SqlError> {
    let lower = raw.to_lowercase();
    if lower.starts_with("if not exists ") {
        return Ok((true, raw["if not exists ".len()..].trim()));
    }
    Ok((false, raw.trim()))
}

fn parse_if_exists(raw: &str) -> Result<(bool, &str), SqlError> {
    let lower = raw.to_lowercase();
    if lower.starts_with("if exists ") {
        return Ok((true, raw["if exists ".len()..].trim()));
    }
    Ok((false, raw.trim()))
}

fn parse_field_definition(raw: &str) -> Result<FieldDefinition, SqlError> {
    let mut parts = raw.split_whitespace();
    let name = parts
        .next()
        .ok_or_else(|| SqlError("invalid column definition".into()))?
        .trim();
    if name.is_empty() {
        return Err(SqlError("invalid column definition".into()));
    }
    let type_tokens: Vec<&str> = parts.collect();
    if type_tokens.is_empty() {
        return Err(SqlError(format!("missing data type for column '{name}'")));
    }
    let data_type = parse_data_type(type_tokens.join(" ").as_str())?;
    Ok(FieldDefinition {
        name: name.to_string(),
        data_type,
    })
}

fn parse_data_type(raw: &str) -> Result<DataType, SqlError> {
    let raw = raw.trim();
    let lower = raw.to_lowercase();
    if let Some(inner) = lower.strip_prefix("vector(") {
        let Some(inner) = inner.strip_suffix(')') else {
            return Err(SqlError(format!("invalid VECTOR type '{raw}'")));
        };
        let dim = inner
            .trim()
            .parse::<usize>()
            .map_err(|_| SqlError(format!("invalid VECTOR dimension '{raw}'")))?;
        if dim == 0 {
            return Err(SqlError(format!("invalid VECTOR dimension '{raw}'")));
        }
        return Ok(DataType::Vector(dim));
    }

    if lower == "int" || lower == "integer" {
        return Ok(DataType::Int);
    }
    if lower == "float" || lower == "double" || lower == "numeric" || lower == "decimal" {
        return Ok(DataType::Float);
    }
    if lower == "boolean" || lower == "bool" {
        return Ok(DataType::Boolean);
    }
    if lower == "json" {
        return Ok(DataType::Json);
    }
    if lower == "text" || lower == "string" {
        return Ok(DataType::Text);
    }

    Err(SqlError(format!("unsupported data type '{raw}'")))
}

fn parse_select_statement(
    sql: &str,
    withs: Vec<CommonTableExpression>,
    recursive: bool,
) -> Result<ParsedStatement, SqlError> {
    if !sql.to_lowercase().starts_with("select ") {
        return Err(SqlError(
            "only SELECT statements are supported in this stage".into(),
        ));
    }

    let trimmed = sql.trim().trim_end_matches(';').trim();
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

    let source = from_tokens[0].trim().to_string();

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
            source: QuerySource::Collection(source),
            ctes: withs,
            recursive,
            projection,
            filter,
            order: order.unwrap_or_default(),
            limit: limit_clause,
            offset: offset_clause,
        }),
    })
}

fn parse_cte_definitions(
    raw: &str,
    recursive: bool,
) -> Result<Vec<CommonTableExpression>, SqlError> {
    let mut out = Vec::new();
    for definition in split_csv(raw) {
        let definition = definition.trim();
        if definition.is_empty() {
            continue;
        }

        let as_pos = find_top_level_keyword(definition, 0, "as").ok_or_else(|| {
            SqlError(format!("invalid CTE definition '{definition}': missing AS"))
        })?;
        let head = definition[..as_pos].trim();
        let body = definition[as_pos + 2..].trim();

        let (name, aliases) = parse_cte_header(head)?;
        let body_sql = parse_enclosed_parenthesized(body)
            .ok_or_else(|| SqlError(format!("invalid CTE body for '{name}'")))?;
        let query = match parse_recursive_cte_query(&body_sql) {
            Some(query) => query,
            None => {
                let parsed_body = parse_statement(&body_sql).map_err(|error| {
                    SqlError(format!("invalid CTE body for '{name}': {}", error.0))
                })?;

                CteQuery::Simple(Box::new(parsed_body))
            }
        };
        if recursive && !matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError(format!(
                "recursive CTE '{name}' must include UNION ALL between anchor and recursive queries"
            )));
        }
        if !recursive && matches!(query, CteQuery::Recursive { .. }) {
            return Err(SqlError("WITH clause is not marked RECURSIVE".to_string()));
        }

        out.push(CommonTableExpression {
            name: name.to_string(),
            aliases,
            query,
        });
    }

    if out.is_empty() {
        return Err(SqlError("empty WITH clause".into()));
    }

    Ok(out)
}

fn parse_recursive_cte_query(body: &str) -> Option<CteQuery> {
    let union_pos = find_top_level_keyword(body, 0, "union all")?;
    let base = body[..union_pos].trim();
    let recursive = body[(union_pos + "union all".len())..].trim();
    if base.is_empty() || recursive.is_empty() {
        return None;
    }

    Some(CteQuery::Recursive {
        base: Box::new(parse_statement(base).ok()?),
        recursive: Box::new(parse_statement(recursive).ok()?),
    })
}

fn parse_cte_header(raw: &str) -> Result<(String, Vec<String>), SqlError> {
    let raw = raw.trim();
    let open = raw.find('(').filter(|open| *open + 1 < raw.len());
    if let Some(open) = open {
        let close = raw
            .rfind(')')
            .ok_or_else(|| SqlError(format!("invalid CTE header '{raw}'")))?;
        if close <= open {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        let name = raw[..open].trim();
        if name.is_empty() || name.contains('(') || name.contains(')') {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        if !raw[close + 1..].trim().is_empty() {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        let aliases = raw[(open + 1)..close]
            .split(',')
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .collect::<Vec<_>>();
        if aliases.is_empty() {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        Ok((name.to_string(), aliases))
    } else {
        if raw.contains('(') || raw.contains(')') {
            return Err(SqlError(format!("invalid CTE header '{raw}'")));
        }

        Ok((raw.to_string(), Vec::new()))
    }
}

fn parse_enclosed_parenthesized(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if !raw.starts_with('(') || !raw.ends_with(')') {
        return None;
    }

    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth == 0 && i != raw.len().saturating_sub(1) {
                    return None;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }

    Some(raw[1..raw.len().saturating_sub(1)].to_string())
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

fn find_top_level_keyword(rest: &str, start: usize, token: &str) -> Option<usize> {
    find_top_level_clause(rest, start, token)
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
