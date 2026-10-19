use super::expr::{parse_expression, split_csv};
use super::identifiers::{normalize_column_identifier, parse_relation_path_prefix};
use super::{
    find_top_level_keyword, parse_statement, Expr, InsertSource, ParsedStatement, QueryStatement,
    SqlError,
};

/// The rows of `INSERT ... VALUES`, checked against `column_count` named
/// target columns (0 when none are named).
fn parse_values_source(raw: &str, column_count: usize) -> Result<Vec<Vec<Expr>>, SqlError> {
    let values_part = super::lexical::trim_separators(raw);
    if values_part.is_empty() {
        return Err(SqlError::new("INSERT requires VALUES list".into()));
    }
    if !values_part.starts_with('(') {
        return Err(SqlError::new(
            "INSERT VALUES requires parenthesized list".into(),
        ));
    }
    let values = parse_insert_values_rows(values_part)?;
    if column_count > 0 {
        if let Some(row) = values.iter().find(|row| row.len() != column_count) {
            return Err(SqlError::new(format!(
                "INSERT column/value counts mismatch: {column_count} columns, {} values",
                row.len()
            )));
        }
    }
    Ok(values)
}

pub(super) fn parse_insert_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let end = super::lexical::pattern_end(trimmed, 0, "insert into")
        .ok_or_else(|| SqlError::new("INSERT requires INTO clause".into()))?;
    let remainder = super::lexical::trim_separators(&trimmed[end..]);
    let (statement_source, returning, returning_aliases) =
        super::dml_targets::split_returning(remainder)?;
    let (statement_source, on_conflict) =
        super::dml_conflict::split_insert_on_conflict(statement_source)?;
    if statement_source.trim().is_empty() {
        return Err(SqlError::new(
            "INSERT requires VALUES or SELECT source".into(),
        ));
    }
    let (table, columns, source, modifiers) = parse_insert_target(statement_source)?;
    let table = table.clone();
    let overriding = modifiers.overriding;
    let alias = modifiers.alias.clone();
    if modifiers.default_values {
        let mut parsed =
            default_values_insert(trimmed, table, source, on_conflict, returning, overriding)?;
        if let QueryStatement::Insert(insert) = &mut parsed.statement {
            insert.alias = alias;
            insert.returning_aliases = returning_aliases;
        }
        return Ok(parsed);
    }

    let values_pos = find_top_level_keyword(source, 0, "values");
    if let Some(values_pos) = values_pos {
        if values_pos != 0 {
            return Err(SqlError::new(
                "INSERT expects VALUES or SELECT at source position".into(),
            ));
        }

        let values = parse_values_source(&source[values_pos + 6..], columns.len())?;

        return Ok(ParsedStatement {
            raw_sql: trimmed.to_string(),
            statement: QueryStatement::Insert(crate::sql::ast::InsertStatement {
                table,
                columns,
                source: InsertSource::Values(values),
                on_conflict,
                returning,
                overriding,
                alias,
                returning_aliases,
                ctes: Vec::new(),
            }),
        });
    }

    let parsed = parse_statement(source)?;
    let QueryStatement::Select(select) = parsed.statement else {
        return Err(SqlError::new(
            "INSERT source must be a SELECT statement".into(),
        ));
    };

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Insert(crate::sql::ast::InsertStatement {
            table,
            columns,
            source: InsertSource::Select(select),
            on_conflict,
            returning,
            overriding,
            alias,
            returning_aliases,
            ctes: Vec::new(),
        }),
    })
}

/// `INSERT INTO t DEFAULT VALUES`: one row of column defaults.
fn default_values_insert(
    trimmed: &str,
    table: crate::sql::ast::IdentifierPath,
    source: &str,
    on_conflict: Option<crate::sql::ast::InsertConflictClause>,
    returning: Vec<crate::sql::ast::SelectItem>,
    overriding: Option<crate::sql::ast::InsertOverriding>,
) -> Result<ParsedStatement, SqlError> {
    if !source.trim().eq_ignore_ascii_case("values") {
        return Err(SqlError::new("DEFAULT VALUES takes no value list".into()));
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Insert(crate::sql::ast::InsertStatement {
            table,
            columns: Vec::new(),
            source: InsertSource::Values(vec![Vec::new()]),
            on_conflict,
            returning,
            overriding,
            alias: None,
            returning_aliases: None,
            ctes: Vec::new(),
        }),
    })
}

pub(super) fn parse_insert_values_rows(values_part: &str) -> Result<Vec<Vec<Expr>>, SqlError> {
    parse_values_rows(values_part, parse_expression)
}

pub(super) fn parse_values_rows(
    values_part: &str,
    parse: fn(&str) -> Result<Expr, SqlError>,
) -> Result<Vec<Vec<Expr>>, SqlError> {
    let mut rows = Vec::new();
    let mut rest = super::lexical::trim_separators(values_part);

    loop {
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with('(') {
            return Err(SqlError::new(
                "INSERT VALUES requires parenthesized list".into(),
            ));
        }

        let close = find_matching_paren(rest, 0)
            .ok_or_else(|| SqlError::new("INSERT VALUES requires closing ')'".into()))?;
        let values_raw = &rest[1..close];
        if values_raw.trim().is_empty() {
            return Err(SqlError::new("INSERT VALUES cannot be empty".into()));
        }

        rows.push(
            split_csv(values_raw)
                .into_iter()
                .map(parse)
                .collect::<Result<Vec<_>, _>>()?,
        );

        rest = super::lexical::trim_separators(&rest[close + 1..]);
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with(',') {
            return Err(SqlError::new(
                "unexpected tokens after INSERT source".into(),
            ));
        }
        rest = rest[1..].trim_start();
        if rest.is_empty() {
            return Err(SqlError::new(
                "INSERT VALUES requires parenthesized list".into(),
            ));
        }
    }

    if rows.is_empty() {
        return Err(SqlError::new("INSERT requires VALUES list".into()));
    }

    Ok(rows)
}

/// Clauses between the column list and the source.
#[derive(Debug, Default)]
pub(super) struct InsertTargetModifiers {
    pub(super) overriding: Option<crate::sql::ast::InsertOverriding>,
    pub(super) default_values: bool,
    pub(super) alias: Option<String>,
}

/// Strips a trailing `OVERRIDING { SYSTEM | USER } VALUE` or `DEFAULT`.
fn strip_target_modifiers(target: &str) -> Result<(&str, InsertTargetModifiers), SqlError> {
    let mut modifiers = InsertTargetModifiers::default();
    let trimmed = target.trim_end();
    let lower = trimmed.to_ascii_lowercase();
    if let Some(head) = lower.strip_suffix("default") {
        if head.ends_with(|character: char| character.is_whitespace() || character == ')') {
            modifiers.default_values = true;
            return Ok((trimmed[..head.len()].trim_end(), modifiers));
        }
    }
    for (suffix, overriding) in [
        ("system value", crate::sql::ast::InsertOverriding::System),
        ("user value", crate::sql::ast::InsertOverriding::User),
    ] {
        let Some(head) = lower.strip_suffix(suffix) else {
            continue;
        };
        let head = head.trim_end();
        let Some(head) = head.strip_suffix("overriding") else {
            return Err(SqlError::new(format!(
                "{} requires OVERRIDING",
                suffix.to_uppercase()
            )));
        };
        modifiers.overriding = Some(overriding);
        return Ok((trimmed[..head.len()].trim_end(), modifiers));
    }
    Ok((trimmed, modifiers))
}

pub(super) fn parse_insert_target(
    raw: &str,
) -> Result<
    (
        crate::sql::ast::IdentifierPath,
        Vec<String>,
        &str,
        InsertTargetModifiers,
    ),
    SqlError,
> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("INSERT INTO requires a table name".into()));
    }

    let values_pos = find_top_level_keyword(raw, 0, "values");
    let select_pos = find_top_level_keyword(raw, 0, "select");
    let source_pos = match (values_pos, select_pos) {
        (Some(values_pos), Some(select_pos)) => Some(values_pos.min(select_pos)),
        (Some(values_pos), None) => Some(values_pos),
        (None, Some(select_pos)) => Some(select_pos),
        (None, None) => None,
    };

    let source_pos = source_pos
        .ok_or_else(|| SqlError::new("INSERT requires VALUES or SELECT source".to_string()))?;

    let (target, modifiers) =
        strip_target_modifiers(super::lexical::trim_separators(&raw[..source_pos]))?;
    let source = raw[source_pos..].trim();
    if source.is_empty() {
        return Err(SqlError::new(
            "INSERT requires VALUES or SELECT source".into(),
        ));
    }
    if target.is_empty() {
        return Err(SqlError::new("INSERT INTO requires a table name".into()));
    }

    let mut modifiers = modifiers;
    let Some(open_paren) = target.find('(') else {
        let (table, extra) = parse_relation_path_prefix(target)?;
        if table.is_empty() {
            return Err(SqlError::new("INSERT INTO requires a table name".into()));
        }
        let (alias, extra) = super::dml_targets::split_target_alias(extra)?;
        if !extra.is_empty() {
            return Err(SqlError::new("INSERT INTO requires a table name".into()));
        }
        modifiers.alias = alias;
        return Ok((table, Vec::new(), source, modifiers));
    };

    let close = find_matching_paren(target, open_paren)
        .ok_or_else(|| SqlError::new("INSERT columns list requires closing ')'".to_string()))?;
    if close < open_paren {
        return Err(SqlError::new(
            "INSERT columns list is malformed".to_string(),
        ));
    }

    let table = target[..open_paren].trim();
    if table.is_empty() {
        return Err(SqlError::new("INSERT INTO requires a table name".into()));
    }
    let (table_path, alias_tail) = parse_relation_path_prefix(table)?;
    let (alias, alias_tail) = super::dml_targets::split_target_alias(alias_tail)?;
    if !alias_tail.is_empty() {
        return Err(SqlError::new("INSERT INTO requires a table name".into()));
    }
    modifiers.alias = alias;

    if !super::lexical::trim_separators(&target[close + 1..]).is_empty() {
        return Err(SqlError::new("INSERT column list is malformed".into()));
    }

    let inside = &target[open_paren + 1..close];
    let columns = split_csv(inside)
        .into_iter()
        .map(|column| {
            let column = column.trim();
            if column.is_empty() {
                return Err(SqlError::new(
                    "INSERT column list cannot include empty columns".into(),
                ));
            }
            normalize_column_identifier(column)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((table_path, columns, source, modifiers))
}

pub(super) fn parse_update_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let mut remainder = super::lexical::trim_separators(&trimmed[6..]);
    if remainder.is_empty() {
        return Err(SqlError::new("UPDATE requires a target table".into()));
    }
    if let Some(end) = super::lexical::pattern_end(remainder, 0, "only") {
        remainder = super::lexical::trim_separators(&remainder[end..]);
    }
    if find_top_level_keyword(remainder, 0, "set") == Some(0)
        || find_top_level_keyword(remainder, 0, "where") == Some(0)
    {
        return Err(SqlError::new("UPDATE requires a table name".into()));
    }
    let (table, tail) = parse_relation_path_prefix(remainder)?;
    if table.is_empty() {
        return Err(SqlError::new("UPDATE requires a target table".into()));
    }
    let (alias, tail) = super::dml_targets::split_target_alias(tail.trim_start_matches('*'))?;
    let set_end = super::lexical::pattern_end(tail, 0, "set")
        .ok_or_else(|| SqlError::new("UPDATE requires SET".into()))?;
    let after_set = super::lexical::trim_separators(&tail[set_end..]);
    let (body, returning, returning_aliases) = super::dml_targets::split_returning(after_set)?;
    let where_pos = find_top_level_keyword(body, 0, "where");
    let from_pos = find_top_level_keyword(body, 0, "from")
        .filter(|from| where_pos.is_none_or(|where_pos| *from < where_pos));
    let set_end = from_pos.or(where_pos).unwrap_or(body.len());
    let parsed = super::dml_targets::parse_set_list(&body[..set_end])?;
    if parsed.assignments.is_empty() && parsed.row_subqueries.is_empty() {
        return Err(SqlError::new(
            "UPDATE SET requires at least one assignment".into(),
        ));
    }
    let from = from_pos
        .map(|from| {
            let end = where_pos.unwrap_or(body.len());
            parse_mutation_source(&body[from + 4..end], "FROM")
        })
        .transpose()?;
    let filter = where_pos
        .map(|position| parse_where(&body[position + 5..]))
        .transpose()?
        .flatten();

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Update(crate::sql::ast::UpdateStatement {
            table,
            assignments: parsed.assignments,
            filter,
            returning,
            alias,
            from,
            row_subqueries: parsed.row_subqueries,
            returning_aliases,
            ctes: Vec::new(),
        }),
    })
}

pub(super) fn parse_delete_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let end = super::lexical::pattern_end(trimmed, 0, "delete from")
        .ok_or_else(|| SqlError::new("DELETE requires FROM clause".into()))?;
    let mut remainder = super::lexical::trim_separators(&trimmed[end..]);
    if let Some(end) = super::lexical::pattern_end(remainder, 0, "only") {
        remainder = super::lexical::trim_separators(&remainder[end..]);
    }
    if remainder.is_empty() {
        return Err(SqlError::new("DELETE requires a target table".into()));
    }

    let (table, tail) = parse_relation_path_prefix(remainder)?;
    if table.is_empty() {
        return Err(SqlError::new("DELETE requires a target table".into()));
    }
    let (alias, tail) = super::dml_targets::split_target_alias(tail.trim_start_matches('*'))?;
    let (body, returning, returning_aliases) = super::dml_targets::split_returning(tail)?;
    let where_pos = find_top_level_keyword(body, 0, "where");
    let using = if let Some(using_end) = super::lexical::pattern_end(body, 0, "using") {
        let end = where_pos.unwrap_or(body.len());
        Some(parse_mutation_source(&body[using_end..end], "USING")?)
    } else {
        if where_pos.is_some_and(|position| position != 0)
            || (where_pos.is_none() && !body.trim().is_empty())
        {
            return Err(SqlError::syntax(format!(
                "syntax error at or near \"{}\"",
                body.split_whitespace().next().unwrap_or_default()
            )));
        }
        None
    };
    let filter = where_pos
        .map(|position| parse_where(&body[position + 5..]))
        .transpose()?
        .flatten();
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Delete(crate::sql::ast::DeleteStatement {
            table,
            filter,
            returning,
            alias,
            using,
            returning_aliases,
            ctes: Vec::new(),
        }),
    })
}

fn parse_where(raw: &str) -> Result<Option<Expr>, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    parse_expression(raw).map(Some)
}

/// Parses a `FROM`/`USING` list; top-level commas are cross joins.
pub(super) fn parse_mutation_source(
    raw: &str,
    clause: &str,
) -> Result<crate::sql::ast::MutationSource, SqlError> {
    let items = split_csv(raw);
    let mut source: Option<crate::sql::ast::QuerySource> = None;
    for item in items {
        let item = super::lexical::trim_separators(item);
        if item.is_empty() {
            return Err(SqlError::syntax(format!(
                "syntax error: {clause} requires a relation"
            )));
        }
        let parsed = super::query::parse_query_source(item)?;
        source = Some(match source {
            None => parsed,
            Some(left) => crate::sql::ast::QuerySource::Join {
                left: Box::new(left),
                right: Box::new(parsed),
                kind: crate::sql::ast::JoinKind::Cross,
                on: Expr::BoolLiteral(true),
            },
        });
    }
    source
        .map(crate::sql::ast::MutationSource::new)
        .ok_or_else(|| SqlError::syntax(format!("syntax error: {clause} requires a relation")))
}

pub(super) fn find_matching_paren(raw: &str, open_at: usize) -> Option<usize> {
    super::lexical::matching_paren(raw, open_at)
}

pub(super) fn split_top_level_assignment(raw: &str) -> Option<usize> {
    super::clauses::find_top_level_keyword_span(raw, 0, "=").map(|(position, _)| position)
}
