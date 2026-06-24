use super::expr::*;
use super::*;

#[path = "schema_references.rs"]
mod schema_references;
use schema_references::parse_references_target;

pub(super) fn parse_create_table_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[12..].trim();

    let (if_not_exists, rest) = parse_if_not_exists(rest)?;

    let open_paren = rest
        .find('(')
        .ok_or_else(|| SqlError("CREATE TABLE requires a column list".into()))?;
    let close_paren = find_matching_paren(rest, open_paren)
        .ok_or_else(|| SqlError("CREATE TABLE requires closing ')'".into()))?;
    if close_paren < open_paren {
        return Err(SqlError("invalid CREATE TABLE definition".into()));
    }

    let table = rest[..open_paren].trim();
    let body = rest[(open_paren + 1)..close_paren].trim();
    let trailing = rest[(close_paren + 1)..].trim();
    if table.is_empty() {
        return Err(SqlError("missing table name".into()));
    }

    let (options, trailing) = parse_index_options(trailing)?;
    if !trailing.is_empty() {
        return Err(SqlError(
            "unexpected tokens after CREATE TABLE columns".into(),
        ));
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

    let storage_mode = match options.get("storage") {
        Some(value) => {
            let Some(mode) = crate::catalog::CollectionStorageMode::parse_option(value) else {
                return Err(SqlError(format!(
                    "unsupported CREATE TABLE storage mode '{value}'"
                )));
            };
            if matches!(mode, crate::catalog::CollectionStorageMode::ColumnIndexed) {
                return Err(SqlError(
                    "CREATE TABLE storage mode 'column_indexed' is derived and cannot be created explicitly"
                        .to_string(),
                ));
            }
            mode
        }
        None => crate::catalog::CollectionStorageMode::RowStore,
    };

    if let Some(key) = options.keys().find(|key| key.as_str() != "storage") {
        return Err(SqlError(format!("unsupported CREATE TABLE option '{key}'")));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateTable(CreateTableStatement {
            table: table.to_string(),
            fields,
            if_not_exists,
            storage_mode,
        }),
    })
}

pub(super) fn parse_create_graph_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed["create graph".len()..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest)?;
    if rest.is_empty() {
        return Err(SqlError("CREATE GRAPH requires a graph name".into()));
    }

    let (name, body) = if let Some(open) = rest.find('(') {
        let close = find_matching_paren(rest, open)
            .ok_or_else(|| SqlError("CREATE GRAPH requires closing ')'".into()))?;
        let name = rest[..open].trim();
        let trailing = rest[(close + 1)..].trim();
        if !trailing.is_empty() {
            return Err(SqlError("unexpected tokens after CREATE GRAPH body".into()));
        }
        (name, Some(rest[(open + 1)..close].trim()))
    } else {
        (rest.trim(), None)
    };

    if name.is_empty() || name.split_whitespace().count() != 1 {
        return Err(SqlError("CREATE GRAPH requires one graph name".into()));
    }

    let mut node_fields = Vec::new();
    let mut edge_fields = Vec::new();
    if let Some(body) = body {
        for section in split_csv(body) {
            let section = section.trim();
            if section.is_empty() {
                continue;
            }
            let lower = section.to_ascii_lowercase();
            let (target, raw_fields) = if lower.starts_with("nodes") {
                ("nodes", section["nodes".len()..].trim())
            } else if lower.starts_with("edges") {
                ("edges", section["edges".len()..].trim())
            } else {
                return Err(SqlError(format!(
                    "unsupported CREATE GRAPH section '{section}'"
                )));
            };
            let fields_body = parse_enclosed_parenthesized(raw_fields).ok_or_else(|| {
                SqlError(format!(
                    "CREATE GRAPH {target} requires fields in parentheses"
                ))
            })?;
            let fields = parse_graph_fields(&fields_body, target)?;
            if target == "nodes" {
                node_fields = fields;
            } else {
                edge_fields = fields;
            }
        }
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateGraph(CreateGraphStatement {
            name: name.to_string(),
            if_not_exists,
            node_fields,
            edge_fields,
        }),
    })
}

fn parse_graph_fields(raw: &str, section: &str) -> Result<Vec<FieldDefinition>, SqlError> {
    let reserved = match section {
        "nodes" => &["node_type", "node_id"][..],
        "edges" => &[
            "edge_id",
            "source_type",
            "source_id",
            "target_type",
            "target_id",
            "edge_type",
            "weight",
        ][..],
        _ => &[][..],
    };
    let mut fields = Vec::new();
    if raw.trim().is_empty() {
        return Ok(fields);
    }
    for field in split_csv(raw) {
        let parsed = parse_field_definition(field.trim())?;
        if reserved
            .iter()
            .any(|reserved| parsed.name.eq_ignore_ascii_case(reserved))
        {
            return Err(SqlError(format!(
                "CREATE GRAPH {section} field '{}' is reserved",
                parsed.name
            )));
        }
        fields.push(parsed);
    }
    Ok(fields)
}

pub(super) fn parse_create_index_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_lowercase();

    let mut unique = false;
    let remainder = if lower.starts_with("create unique index ") {
        unique = true;
        &trimmed["create unique index ".len()..]
    } else if lower.starts_with("create index ") {
        &trimmed["create index ".len()..]
    } else {
        return Err(SqlError("unsupported CREATE INDEX statement".to_string()));
    };

    if starts_with_keyword(remainder, "concurrently") {
        return Err(SqlError(
            "CREATE INDEX CONCURRENTLY is not supported in this version".to_string(),
        ));
    }

    let (if_not_exists, remainder) = parse_if_not_exists(remainder)?;

    let on_pos = find_top_level_keyword(remainder, 0, "on")
        .ok_or_else(|| SqlError("CREATE INDEX requires 'ON' clause".to_string()))?;

    let name = remainder[..on_pos].trim();
    if name.is_empty() {
        return Err(SqlError("CREATE INDEX missing index name".to_string()));
    }

    let remainder = remainder[on_pos + 2..].trim();
    let (table, remainder) = parse_index_target(remainder)?;
    let (kind, remainder) = parse_index_kind(remainder)?;
    let (fields, expressions, remainder) = parse_index_fields(remainder)?;
    let (include_fields, remainder) = parse_index_include_fields(remainder)?;
    let (predicate, remainder) = parse_index_predicate(remainder)?;
    let (options, remainder) = parse_index_options(remainder)?;

    if !remainder.is_empty() {
        return Err(SqlError("unsupported CREATE INDEX syntax".to_string()));
    }

    if !matches!(kind, IndexKind::Scalar | IndexKind::Column)
        && fields.len() + expressions.len() > 1
    {
        return Err(SqlError(
            "composite indexes are only supported for scalar index methods".to_string(),
        ));
    }
    if !matches!(kind, IndexKind::Scalar) && !expressions.is_empty() {
        return Err(SqlError(
            "expression indexes are only supported for scalar index methods".to_string(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateIndex(CreateIndexStatement {
            name: name.to_string(),
            table: table.to_string(),
            fields,
            expressions,
            include_fields,
            predicate,
            if_not_exists,
            unique,
            kind,
            options,
        }),
    })
}

pub(super) fn parse_drop_index_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[10..].trim();

    let (if_exists, rest) = parse_if_exists(rest)?;
    let on_pos = find_top_level_keyword(rest, 0, "on")
        .ok_or_else(|| SqlError("DROP INDEX requires 'ON' clause".to_string()))?;

    let name = rest[..on_pos].trim();
    let table = rest[on_pos + 2..].trim();
    if name.is_empty() || table.is_empty() {
        return Err(SqlError(
            "DROP INDEX requires index name and table".to_string(),
        ));
    }
    if table.contains(' ') {
        return Err(SqlError(
            "unsupported tokens after DROP INDEX table name".to_string(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropIndex(DropIndexStatement {
            name: name.to_string(),
            table: table.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_drop_table_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
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

pub(super) fn parse_drop_schema_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[11..].trim();

    let (if_exists, rest) = parse_if_exists(rest)?;
    let schema = rest.trim();
    if schema.is_empty() {
        return Err(SqlError("missing schema name in DROP SCHEMA".into()));
    }
    if schema.split_whitespace().count() != 1 {
        return Err(SqlError(
            "DROP SCHEMA supports only a single schema name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropSchema(DropSchemaStatement {
            schema: schema.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_alter_table_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
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

pub(super) fn parse_alter_table_operation(raw: &str) -> Result<AlterTableOperation, SqlError> {
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
    if lower.starts_with("rename column") {
        let rest = raw["rename column".len()..].trim();
        let (from, to) = split_keyword(rest, "to")
            .ok_or_else(|| SqlError("RENAME COLUMN requires TO clause".into()))?;
        if from.split_whitespace().count() != 1 {
            return Err(SqlError(
                "RENAME COLUMN supports only one source column".into(),
            ));
        }
        if to.split_whitespace().count() != 1 {
            return Err(SqlError(
                "RENAME COLUMN supports only one target column".into(),
            ));
        }
        return Ok(AlterTableOperation::RenameColumn {
            from: from.to_string(),
            to: to.to_string(),
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

pub(super) fn parse_create_schema_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
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

pub(super) fn parse_alter_schema_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[12..].trim();

    let (schema, rest) = split_first_token(rest)
        .ok_or_else(|| SqlError("missing schema name in ALTER SCHEMA".into()))?;
    if schema.trim().is_empty() {
        return Err(SqlError("missing schema name in ALTER SCHEMA".into()));
    }

    let rest = rest.trim();
    let lower = rest.to_lowercase();
    if !lower.starts_with("rename to") {
        return Err(SqlError("unsupported ALTER SCHEMA operation".into()));
    }

    let target = rest["rename to".len()..].trim();
    if target.is_empty() {
        return Err(SqlError("RENAME TO requires a schema name".into()));
    }
    if target.split_whitespace().count() != 1 {
        return Err(SqlError("RENAME TO supports only one schema name".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterSchema(AlterSchemaStatement {
            schema: schema.to_string(),
            operation: AlterSchemaOperation::RenameTo {
                schema: target.to_string(),
            },
        }),
    })
}

pub(super) fn parse_create_role_statement(
    sql: &str,
    user_alias: bool,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[11..].trim();
    let (if_not_exists, rest) = parse_if_not_exists(rest)?;
    let mut tokens = tokenize_schema_field(rest).into_iter();

    let name = tokens
        .next()
        .ok_or_else(|| SqlError("missing role name".into()))?;
    if name.trim().is_empty() {
        return Err(SqlError("missing role name".into()));
    }

    let mut login = user_alias;
    let mut password = None;
    while let Some(token) = tokens.next() {
        match token.to_ascii_lowercase().as_str() {
            "login" => login = true,
            "nologin" => login = false,
            "password" => {
                let raw_password = tokens
                    .next()
                    .ok_or_else(|| SqlError("PASSWORD requires a value".into()))?;
                password = parse_optional_role_password(&raw_password)?;
            }
            other => {
                return Err(SqlError(format!(
                    "unsupported CREATE ROLE option '{other}'"
                )));
            }
        }
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::CreateRole(CreateRoleStatement {
            name,
            if_not_exists,
            login,
            password,
        }),
    })
}

pub(super) fn parse_alter_role_statement(
    sql: &str,
    _user_alias: bool,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[10..].trim();
    let (name, rest) =
        split_first_token(rest).ok_or_else(|| SqlError("missing role name".into()))?;
    let mut tokens = tokenize_schema_field(rest).into_iter();
    let mut login = None;
    let mut password = None;

    while let Some(token) = tokens.next() {
        match token.to_ascii_lowercase().as_str() {
            "login" => login = Some(true),
            "nologin" => login = Some(false),
            "password" => {
                let raw_password = tokens
                    .next()
                    .ok_or_else(|| SqlError("PASSWORD requires a value".into()))?;
                password = parse_optional_role_password(&raw_password)?;
            }
            other => {
                return Err(SqlError(format!("unsupported ALTER ROLE option '{other}'")));
            }
        }
    }

    if login.is_none() && password.is_none() {
        return Err(SqlError("ALTER ROLE requires at least one option".into()));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::AlterRole(AlterRoleStatement {
            name,
            login,
            password,
        }),
    })
}

pub(super) fn parse_drop_role_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let rest = trimmed[9..].trim();
    let (if_exists, rest) = parse_if_exists(rest)?;
    let role = rest.trim();
    if role.is_empty() {
        return Err(SqlError("missing role name".into()));
    }
    if role.split_whitespace().count() != 1 {
        return Err(SqlError(
            "DROP ROLE supports only a single role name".into(),
        ));
    }

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::DropRole(DropRoleStatement {
            name: role.to_string(),
            if_exists,
        }),
    })
}

pub(super) fn parse_if_not_exists(raw: &str) -> Result<(bool, &str), SqlError> {
    let lower = raw.to_lowercase();
    if lower.starts_with("if not exists ") {
        return Ok((true, raw["if not exists ".len()..].trim()));
    }
    Ok((false, raw.trim()))
}

pub(super) fn parse_if_exists(raw: &str) -> Result<(bool, &str), SqlError> {
    let lower = raw.to_lowercase();
    if lower.starts_with("if exists ") {
        return Ok((true, raw["if exists ".len()..].trim()));
    }
    Ok((false, raw.trim()))
}

pub(super) fn split_first_token(raw: &str) -> Option<(String, &str)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let first = parts.next()?.trim();
    if first.is_empty() {
        return None;
    }

    Some((first.to_string(), parts.next().unwrap_or("").trim_start()))
}

pub(super) fn parse_field_definition(raw: &str) -> Result<FieldDefinition, SqlError> {
    let mut parts = tokenize_schema_field(raw).into_iter();
    let name = parts
        .next()
        .ok_or_else(|| SqlError("invalid column definition".into()))?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(SqlError("invalid column definition".into()));
    }
    let type_token = parts
        .next()
        .ok_or_else(|| SqlError(format!("missing data type for column '{name}'")))?
        .trim()
        .to_string();
    if type_token.is_empty() {
        return Err(SqlError(format!("missing data type for column '{name}'")));
    }
    let data_type = parse_data_type(&type_token)?;

    let mut constraint = FieldConstraint {
        field: name.clone(),
        not_null: false,
        unique: false,
        primary_key: false,
        default_value: None,
        check: None,
        references_table: None,
        references_field: None,
    };

    let mut saw_constraint = false;
    while let Some(token) = parts.next() {
        match token.to_lowercase().as_str() {
            "not" => {
                let next = parts
                    .next()
                    .ok_or_else(|| SqlError("NOT must be followed by NULL".into()))?;
                if !next.eq_ignore_ascii_case("null") {
                    return Err(SqlError(format!("unsupported constraint '{token} {next}'")));
                }
                saw_constraint = true;
                constraint.not_null = true;
            }
            "null" => {
                return Err(SqlError("unexpected NULL constraint".to_string()));
            }
            "unique" => {
                saw_constraint = true;
                constraint.unique = true;
            }
            "primary" => {
                let next = parts
                    .next()
                    .ok_or_else(|| SqlError("PRIMARY must be followed by KEY".into()))?;
                if !next.eq_ignore_ascii_case("key") {
                    return Err(SqlError(format!("unsupported constraint '{token} {next}'")));
                }
                saw_constraint = true;
                constraint.primary_key = true;
            }
            "key" => {
                return Err(SqlError("KEY without PRIMARY".to_string()));
            }
            "default" => {
                saw_constraint = true;
                let value = parts
                    .next()
                    .ok_or_else(|| SqlError("DEFAULT requires a value".into()))?;
                constraint.default_value = Some(parse_constraint_literal(&value)?);
            }
            "check" => {
                saw_constraint = true;
                let expression = parts
                    .next()
                    .ok_or_else(|| SqlError("CHECK requires an expression".into()))?;
                let remaining = parts.collect::<Vec<_>>().join(" ");
                let expression = if remaining.is_empty() {
                    expression
                } else {
                    format!("{expression} {remaining}")
                };
                let constraint_check = parse_check_constraint(&expression)?;
                constraint.check = Some(constraint_check);
                break;
            }
            "references" => {
                saw_constraint = true;
                let reference = parts.next().ok_or_else(|| {
                    SqlError("REFERENCES requires target table and column".into())
                })?;
                let (table, field) = parse_references_target(&reference)?;
                constraint.references_table = Some(table);
                constraint.references_field = Some(field);
            }
            other => {
                return Err(SqlError(format!("unsupported constraint '{other}'")));
            }
        }
    }

    if !saw_constraint {
        return Ok(FieldDefinition {
            name: name.to_string(),
            data_type,
            constraints: Vec::new(),
        });
    }

    Ok(FieldDefinition {
        name: name.to_string(),
        data_type,
        constraints: vec![constraint],
    })
}

pub(super) fn parse_check_constraint(raw: &str) -> Result<ConstraintCheck, SqlError> {
    let expression = raw.trim();
    if !expression.starts_with('(') || !expression.ends_with(')') {
        return Err(SqlError(
            "CHECK expression must be parenthesized".to_string(),
        ));
    }
    let inner = strip_parentheses(expression)
        .ok_or_else(|| SqlError("invalid CHECK expression".to_string()))?
        .trim();

    let (left, op, right) = parse_simple_comparison(inner)
        .ok_or_else(|| SqlError("unsupported CHECK expression".to_string()))?;

    if !left
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(SqlError(
            "CHECK expression field must be an identifier".to_string(),
        ));
    }

    Ok(ConstraintCheck {
        field: left.to_string(),
        operator: op,
        value: right,
    })
}

pub(super) fn parse_simple_comparison(raw: &str) -> Option<(String, ConstraintOperator, Value)> {
    let candidates = [
        (" <=", ConstraintOperator::Lte),
        (" >=", ConstraintOperator::Gte),
        (" <> ", ConstraintOperator::NotEq),
        (" != ", ConstraintOperator::NotEq),
        (" like ", ConstraintOperator::Like),
        (" < ", ConstraintOperator::Lt),
        (" > ", ConstraintOperator::Gt),
        (" = ", ConstraintOperator::Eq),
    ];

    for (operator, kind) in candidates {
        let lower = raw.to_lowercase();
        if let Some(position) = lower.find(operator) {
            let left = raw[..position].trim();
            let right = raw[position + operator.len()..].trim();
            if left.is_empty() || right.is_empty() {
                continue;
            }

            if let Ok(value) = parse_constraint_literal(right) {
                return Some((left.to_string(), kind, value));
            }
        }
    }

    None
}

pub(super) fn parse_constraint_literal(raw: &str) -> Result<Value, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError("invalid literal".to_string()));
    }

    if raw.eq_ignore_ascii_case("null") {
        return Ok(Value::Null);
    }
    if raw.eq_ignore_ascii_case("true") {
        return Ok(Value::Bool(true));
    }
    if raw.eq_ignore_ascii_case("false") {
        return Ok(Value::Bool(false));
    }

    if let Some(rest) = raw.strip_prefix('\'') {
        if raw.ends_with('\'') && raw.len() >= 2 {
            let unquoted = rest.strip_suffix('\'').unwrap_or(rest);
            return Ok(Value::String(unquoted.to_string()));
        }
    }
    if let Some(rest) = raw.strip_prefix('"') {
        if raw.ends_with('"') && raw.len() >= 2 {
            let unquoted = rest.strip_suffix('"').unwrap_or(rest);
            return Ok(Value::String(unquoted.to_string()));
        }
    }

    if let Ok(value) = raw.parse::<i64>() {
        return Ok(value.into());
    }
    if let Ok(value) = raw.parse::<f64>() {
        return Ok(value.into());
    }

    Ok(Value::String(raw.to_string()))
}

pub(super) fn tokenize_schema_field(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0isize;
    let mut start = 0usize;

    for (idx, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth -= 1,
            ' ' if !in_single && !in_double && depth == 0 => {
                if start != idx {
                    out.push(raw[start..idx].to_string());
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if start < raw.len() {
        out.push(raw[start..].to_string());
    }

    out.into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn parse_index_target(raw: &str) -> Result<(String, &str), SqlError> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(SqlError("missing table name in CREATE INDEX".to_string()));
    }

    let table = tokens[0];
    if table.contains('(') || table.contains(')') {
        return Err(SqlError("invalid table name in CREATE INDEX".to_string()));
    }

    let remainder = raw[table.len()..].trim_start();
    Ok((table.to_string(), remainder))
}

pub(super) fn parse_index_kind(raw: &str) -> Result<(IndexKind, &str), SqlError> {
    if !starts_with_keyword(raw, "using") {
        return Ok((IndexKind::Scalar, raw));
    }

    let remainder = raw[5..].trim_start();
    if starts_with_keyword(remainder, "btree") {
        return Ok((IndexKind::Scalar, remainder[5..].trim_start()));
    }
    if starts_with_keyword(remainder, "hash") {
        return Ok((IndexKind::Scalar, remainder[4..].trim_start()));
    }
    if starts_with_keyword(remainder, "gin") {
        return Ok((IndexKind::FullText, remainder[3..].trim_start()));
    }
    if starts_with_keyword(remainder, "fulltext") {
        return Ok((IndexKind::FullText, remainder[8..].trim_start()));
    }
    if starts_with_keyword(remainder, "vector") {
        return Ok((IndexKind::Vector, remainder[6..].trim_start()));
    }
    if starts_with_keyword(remainder, "column") {
        return Ok((IndexKind::Column, remainder[6..].trim_start()));
    }
    if starts_with_keyword(remainder, "time_series") {
        return Ok((IndexKind::TimeSeries, remainder[11..].trim_start()));
    }
    if starts_with_keyword(remainder, "timeseries") {
        return Ok((IndexKind::TimeSeries, remainder[10..].trim_start()));
    }

    Err(SqlError("unsupported index method".to_string()))
}

pub(super) fn parse_index_fields(raw: &str) -> Result<(Vec<String>, Vec<Expr>, &str), SqlError> {
    let raw = raw.trim();
    if !raw.starts_with('(') {
        return Err(SqlError(
            "CREATE INDEX requires indexed field list in parentheses".to_string(),
        ));
    }

    let close = find_matching_paren(raw, 0)
        .ok_or_else(|| SqlError("CREATE INDEX field list missing closing ')'".to_string()))?;
    let field_spec = &raw[1..close];
    if field_spec.trim().is_empty() {
        return Err(SqlError("CREATE INDEX field cannot be empty".to_string()));
    }

    let before = raw[close + 1..].trim();
    let mut fields = Vec::new();
    let mut expressions = Vec::new();
    for field in split_csv(field_spec) {
        let field = field.trim();
        if field.is_empty() {
            return Err(SqlError("CREATE INDEX field cannot be empty".to_string()));
        }
        if is_index_field_identifier(field) {
            fields.push(field.to_string());
        } else {
            if field.contains(';') {
                return Err(SqlError("invalid expression index definition".to_string()));
            }
            expressions.push(parse_expression(field)?);
        }
    }

    Ok((fields, expressions, before))
}

pub(super) fn is_index_field_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(super) fn parse_index_include_fields(raw: &str) -> Result<(Vec<String>, &str), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() || !starts_with_keyword(raw, "include") {
        return Ok((Vec::new(), raw));
    }

    let body = raw["include".len()..].trim_start();
    if !body.starts_with('(') {
        return Err(SqlError(
            "INCLUDE requires field list in parentheses".to_string(),
        ));
    }
    let close = body
        .find(')')
        .ok_or_else(|| SqlError("INCLUDE field list missing closing ')'".to_string()))?;
    let field_spec = &body[1..close];
    if field_spec.trim().is_empty() {
        return Err(SqlError("INCLUDE field cannot be empty".to_string()));
    }

    let mut fields = Vec::new();
    for field in split_csv(field_spec) {
        let field = field.trim();
        if field.is_empty() {
            return Err(SqlError("INCLUDE field cannot be empty".to_string()));
        }
        if field
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '(' | ')' | ';'))
        {
            return Err(SqlError(
                "expression INCLUDE definitions are not supported".to_string(),
            ));
        }
        fields.push(field.to_string());
    }

    Ok((fields, body[close + 1..].trim_start()))
}

pub(super) fn parse_index_predicate(raw: &str) -> Result<(Option<Expr>, &str), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() || !starts_with_keyword(raw, "where") {
        return Ok((None, raw));
    }
    let predicate = raw["where".len()..].trim();
    if predicate.is_empty() {
        return Err(SqlError(
            "CREATE INDEX WHERE requires predicate".to_string(),
        ));
    }
    Ok((Some(parse_expression(predicate)?), ""))
}

pub(super) fn parse_index_options(
    raw: &str,
) -> Result<(std::collections::BTreeMap<String, String>, &str), SqlError> {
    let mut options = std::collections::BTreeMap::new();
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((options, raw));
    }

    if !starts_with_keyword(raw, "with") {
        return Ok((options, raw));
    }

    let with_body = raw[4..].trim_start();
    if !with_body.starts_with('(') || !with_body.ends_with(')') {
        return Err(SqlError(
            "WITH options must be enclosed in parentheses".to_string(),
        ));
    }

    let body = &with_body[1..with_body.len() - 1];
    for token in split_csv(body) {
        let token = token.trim();
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| SqlError("index option must be key=value".to_string()))?;
        let key = key.trim().to_lowercase();
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if key.is_empty() {
            return Err(SqlError("index option key cannot be empty".to_string()));
        }
        options.insert(key, value);
    }

    Ok((options, ""))
}

pub(super) fn starts_with_keyword(raw: &str, keyword: &str) -> bool {
    let lower = raw.to_lowercase();
    if !lower.starts_with(keyword) {
        return false;
    }

    let suffix = lower.chars().nth(keyword.len()).unwrap_or(' ');
    !suffix.is_ascii_alphanumeric()
}

pub(super) fn parse_data_type(raw: &str) -> Result<DataType, SqlError> {
    DataType::parse_sql(raw).map_err(SqlError)
}
