use super::expr::{parse_expr_token, parse_expression, split_csv};
use super::identifiers::{normalize_identifier, parse_relation_path_prefix};
use super::query::parse_projection_items;
use super::{
    find_top_level_keyword, parse_statement, strip_parentheses, Expr, InsertSource,
    ParsedStatement, QueryStatement, SelectItem, SqlError,
};

pub(super) fn parse_insert_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let end = super::lexical::pattern_end(trimmed, 0, "insert into")
        .ok_or_else(|| SqlError::new("INSERT requires INTO clause".into()))?;
    let remainder = super::lexical::trim_separators(&trimmed[end..]);
    let (statement_source, returning) = split_statement_and_returning(remainder)?;
    let (statement_source, on_conflict) = split_insert_on_conflict(statement_source)?;
    if statement_source.trim().is_empty() {
        return Err(SqlError::new(
            "INSERT requires VALUES or SELECT source".into(),
        ));
    }
    let (table, columns, source) = parse_insert_target(statement_source)?;
    let table = table.clone();

    let values_pos = find_top_level_keyword(source, 0, "values");
    if let Some(values_pos) = values_pos {
        if values_pos != 0 {
            return Err(SqlError::new(
                "INSERT expects VALUES or SELECT at source position".into(),
            ));
        }

        let values_part = super::lexical::trim_separators(&source[values_pos + 6..]);
        if values_part.is_empty() {
            return Err(SqlError::new("INSERT requires VALUES list".into()));
        }
        if !values_part.starts_with('(') {
            return Err(SqlError::new(
                "INSERT VALUES requires parenthesized list".into(),
            ));
        }

        let values = parse_insert_values_rows(values_part)?;

        if !columns.is_empty() {
            for row in &values {
                if columns.len() != row.len() {
                    return Err(SqlError::new(format!(
                        "INSERT column/value counts mismatch: {} columns, {} values",
                        columns.len(),
                        row.len()
                    )));
                }
            }
        }

        return Ok(ParsedStatement {
            raw_sql: trimmed.to_string(),
            statement: QueryStatement::Insert(crate::sql::ast::InsertStatement {
                table,
                columns,
                source: InsertSource::Values(values),
                on_conflict,
                returning,
            }),
        });
    }

    if returning.is_empty() {
        // Keep parsing behavior consistent for select sources with explicit and implicit `RETURNING`.
        let parsed = parse_statement(source)?;
        let QueryStatement::Select(select) = parsed.statement else {
            return Err(SqlError::new(
                "INSERT source must be a SELECT statement".into(),
            ));
        };

        return Ok(ParsedStatement {
            raw_sql: trimmed.to_string(),
            statement: QueryStatement::Insert(crate::sql::ast::InsertStatement {
                table,
                columns,
                source: InsertSource::Select(Box::new(select)),
                on_conflict,
                returning: Vec::new(),
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
            source: InsertSource::Select(Box::new(select)),
            on_conflict,
            returning,
        }),
    })
}

fn split_insert_on_conflict(
    raw: &str,
) -> Result<(&str, Option<crate::sql::ast::InsertConflictClause>), SqlError> {
    let Some(pos) = find_top_level_keyword(raw, 0, "on conflict") else {
        return Ok((raw, None));
    };
    let statement = raw[..pos].trim();
    let clause = raw[pos..].trim();
    Ok((statement, Some(parse_on_conflict_clause(clause)?)))
}

fn parse_on_conflict_clause(raw: &str) -> Result<crate::sql::ast::InsertConflictClause, SqlError> {
    let end = super::lexical::pattern_end(raw, 0, "on conflict")
        .ok_or_else(|| SqlError::new("invalid ON CONFLICT clause".into()))?;
    let remainder = super::lexical::trim_separators(&raw[end..]);
    let do_pos = find_top_level_keyword(remainder, 0, "do")
        .ok_or_else(|| SqlError::new("ON CONFLICT requires DO clause".into()))?;
    let target_raw = super::lexical::trim_separators(&remainder[..do_pos]);
    let action_raw = remainder[do_pos + 2..].trim();

    let target_fields = if target_raw.is_empty() {
        Vec::new()
    } else {
        let fields = strip_parentheses(target_raw)
            .ok_or_else(|| SqlError::new("ON CONFLICT target must be parenthesized".into()))?;
        split_csv(fields)
            .into_iter()
            .map(|field| {
                let field = super::lexical::trim_separators(field);
                if field.is_empty() {
                    Err(SqlError::new(
                        "ON CONFLICT target field cannot be empty".into(),
                    ))
                } else {
                    normalize_identifier(field)
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let action = parse_on_conflict_action(action_raw)?;
    Ok(crate::sql::ast::InsertConflictClause {
        target_fields,
        action,
    })
}

fn parse_on_conflict_action(raw: &str) -> Result<crate::sql::ast::InsertConflictAction, SqlError> {
    let raw = super::lexical::trim_separators(raw);
    let lower = raw.to_ascii_lowercase();
    if lower == "nothing" {
        return Ok(crate::sql::ast::InsertConflictAction::DoNothing);
    }
    let end = super::lexical::pattern_end(raw, 0, "update set")
        .ok_or_else(|| SqlError::new("ON CONFLICT supports DO NOTHING or DO UPDATE SET".into()))?;
    let after_set = super::lexical::trim_separators(&raw[end..]);
    let (assignments_raw, trailing) = split_trailing_update_clauses(after_set)?;
    let assignments = parse_assignment_list(assignments_raw)?;
    if assignments.is_empty() {
        return Err(SqlError::new(
            "ON CONFLICT DO UPDATE SET requires at least one assignment".into(),
        ));
    }
    let filter = if trailing.trim().is_empty() {
        None
    } else {
        let (filter, returning) = parse_filter_and_returning(trailing)?;
        if !returning.is_empty() {
            return Err(SqlError::new(
                "ON CONFLICT DO UPDATE cannot contain RETURNING inside clause".into(),
            ));
        }
        filter
    };

    Ok(crate::sql::ast::InsertConflictAction::DoUpdate {
        assignments,
        filter,
    })
}

pub(super) fn split_statement_and_returning(
    raw: &str,
) -> Result<(&str, Vec<SelectItem>), SqlError> {
    let raw = raw.trim();
    let returning_pos = find_top_level_keyword(raw, 0, "returning");
    if let Some(pos) = returning_pos {
        let source = raw[..pos].trim();
        let returning = parse_projection_items(&raw[pos + 9..])?;
        Ok((source, returning))
    } else {
        Ok((raw, Vec::new()))
    }
}

pub(super) fn parse_insert_values_rows(values_part: &str) -> Result<Vec<Vec<Expr>>, SqlError> {
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
                .map(parse_expr_token)
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

pub(super) fn parse_insert_target(
    raw: &str,
) -> Result<(crate::sql::ast::IdentifierPath, Vec<String>, &str), SqlError> {
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

    let target = super::lexical::trim_separators(&raw[..source_pos]);
    let source = raw[source_pos..].trim();
    if source.is_empty() {
        return Err(SqlError::new(
            "INSERT requires VALUES or SELECT source".into(),
        ));
    }
    if target.is_empty() {
        return Err(SqlError::new("INSERT INTO requires a table name".into()));
    }

    let Some(open_paren) = target.find('(') else {
        let (table, extra) = parse_relation_path_prefix(target)?;
        if table.is_empty() {
            return Err(SqlError::new("INSERT INTO requires a table name".into()));
        }
        if !extra.is_empty() {
            return Err(SqlError::new("INSERT INTO requires a table name".into()));
        }
        return Ok((table, Vec::new(), source));
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

    if !target[close + 1..].starts_with(' ') && target[close + 1..].chars().next().is_some() {
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
            normalize_identifier(column)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((
        crate::sql::ast::IdentifierPath::parse(table).map_err(SqlError::new)?,
        columns,
        source,
    ))
}

pub(super) fn parse_update_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let remainder = super::lexical::trim_separators(&trimmed[6..]);
    if remainder.is_empty() {
        return Err(SqlError::new("UPDATE requires a target table".into()));
    }

    let set_pos = find_top_level_keyword(remainder, 0, "set")
        .ok_or_else(|| SqlError::new("UPDATE requires SET".into()))?;
    if find_top_level_keyword(remainder, 0, "from").is_some() {
        return Err(SqlError::new(
            "UPDATE FROM is not supported in this version".into(),
        ));
    }
    if find_top_level_keyword(remainder, 0, "where") == Some(0) {
        return Err(SqlError::new("UPDATE requires a table name".into()));
    }

    let table = super::lexical::trim_separators(&remainder[..set_pos]);
    if table.is_empty() {
        return Err(SqlError::new("UPDATE requires a target table".into()));
    }

    let after_set = super::lexical::trim_separators(&remainder[(set_pos + 3)..]);
    let (set_clause, remaining) = split_trailing_update_clauses(after_set)?;
    let assignments = parse_assignment_list(set_clause)?;
    if assignments.is_empty() {
        return Err(SqlError::new(
            "UPDATE SET requires at least one assignment".into(),
        ));
    }
    let (filter, returning) = parse_filter_and_returning(remaining)?;

    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Update(crate::sql::ast::UpdateStatement {
            table: crate::sql::ast::IdentifierPath::parse(table).map_err(SqlError::new)?,
            assignments,
            filter,
            returning,
        }),
    })
}

pub(super) fn parse_delete_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let end = super::lexical::pattern_end(trimmed, 0, "delete from")
        .ok_or_else(|| SqlError::new("DELETE requires FROM clause".into()))?;
    let remainder = super::lexical::trim_separators(&trimmed[end..]);
    if remainder.is_empty() {
        return Err(SqlError::new("DELETE requires a target table".into()));
    }

    let (table, tail) = parse_relation_path_prefix(remainder)?;
    if table.is_empty() {
        return Err(SqlError::new("DELETE requires a target table".into()));
    }

    let (filter, returning) = parse_filter_and_returning(tail)?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Delete(crate::sql::ast::DeleteStatement {
            table,
            filter,
            returning,
        }),
    })
}

pub(super) fn parse_filter_and_returning(
    raw: &str,
) -> Result<(Option<Expr>, Vec<crate::sql::ast::SelectItem>), SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((None, Vec::new()));
    }

    let where_pos = find_top_level_keyword(raw, 0, "where");
    let returning_pos = find_top_level_keyword(raw, 0, "returning");

    match (where_pos, returning_pos) {
        (Some(where_pos), Some(returning_pos)) if where_pos > returning_pos => {
            Err(SqlError::new("unexpected RETURNING order".into()))
        }
        (Some(where_pos), Some(returning_pos)) => {
            let filter_raw = raw[where_pos + 5..returning_pos].trim();
            let returning_raw = raw[returning_pos + 9..].trim();
            let filter = if filter_raw.is_empty() {
                None
            } else {
                Some(parse_expression(filter_raw)?)
            };
            let returning = parse_projection_items(returning_raw)?;
            Ok((filter, returning))
        }
        (Some(where_pos), None) => {
            let filter_raw = raw[where_pos + 5..].trim();
            let filter = if filter_raw.is_empty() {
                None
            } else {
                Some(parse_expression(filter_raw)?)
            };
            Ok((filter, Vec::new()))
        }
        (None, Some(returning_pos)) => {
            let filter = None;
            let returning_raw = raw[returning_pos + 9..].trim();
            let returning = parse_projection_items(returning_raw)?;
            Ok((filter, returning))
        }
        (None, None) => Ok((None, Vec::new())),
    }
}

pub(super) fn parse_assignment_list(raw: &str) -> Result<Vec<(String, Expr)>, SqlError> {
    let mut assignments = Vec::new();
    if raw.trim().is_empty() {
        return Ok(assignments);
    }

    for assignment in split_csv(raw) {
        let assignment = assignment.trim();
        if assignment.is_empty() {
            return Err(SqlError::new(
                "UPDATE SET cannot include empty assignment".into(),
            ));
        }
        assignments.push(parse_assignment(assignment)?);
    }

    Ok(assignments)
}

pub(super) fn find_matching_paren(raw: &str, open_at: usize) -> Option<usize> {
    super::lexical::matching_paren(raw, open_at)
}

pub(super) fn split_trailing_update_clauses(raw: &str) -> Result<(&str, &str), SqlError> {
    let where_pos = find_top_level_keyword(raw, 0, "where");
    let returning_pos = find_top_level_keyword(raw, 0, "returning");

    match (where_pos, returning_pos) {
        (None, None) => Ok((raw, "")),
        (Some(where_pos), Some(returning_pos)) if where_pos < returning_pos => {
            Ok((&raw[..where_pos], &raw[where_pos..]))
        }
        (Some(_), Some(_)) => Err(SqlError::new("unexpected RETURNING order".into())),
        (Some(pos), None) | (None, Some(pos)) => Ok((&raw[..pos], &raw[pos..])),
    }
}

pub(super) fn parse_assignment(raw: &str) -> Result<(String, Expr), SqlError> {
    let eq_pos = split_top_level_assignment(raw)
        .ok_or_else(|| SqlError::new("UPDATE SET assignments require '='".into()))?;

    let (left, right) = raw.split_at(eq_pos);
    let left = super::lexical::trim_separators(left);
    if left.is_empty() {
        return Err(SqlError::new(
            "UPDATE SET assignment missing column name".into(),
        ));
    }
    let right = right[1..].trim();
    if right.is_empty() {
        return Err(SqlError::new("UPDATE SET assignment missing value".into()));
    }

    Ok((normalize_identifier(left)?, parse_expression(right)?))
}

pub(super) fn split_top_level_assignment(raw: &str) -> Option<usize> {
    super::clauses::find_top_level_keyword_span(raw, 0, "=").map(|(position, _)| position)
}
