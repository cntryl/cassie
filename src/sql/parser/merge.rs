//! `[WITH ...] MERGE INTO target [AS alias] USING source ON condition
//! WHEN [NOT] MATCHED [BY SOURCE | BY TARGET] [AND condition] THEN action ...
//! [RETURNING ...]`.

use super::expr::{parse_expression, split_csv};
use super::identifiers::{normalize_column_identifier, parse_relation_path_prefix};
use super::lexical::{matching_paren, pattern_end, trim_separators};
use super::{
    find_top_level_keyword, CommonTableExpression, ParsedStatement, QueryStatement, SqlError,
};
use crate::sql::ast::{InsertOverriding, MergeAction, MergeClause, MergeMatch, MergeStatement};

pub(super) fn parse_merge_statement(
    sql: &str,
    ctes: Vec<CommonTableExpression>,
) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let end = pattern_end(trimmed, 0, "merge into")
        .ok_or_else(|| SqlError::syntax("MERGE requires INTO".into()))?;
    let mut rest = trim_separators(&trimmed[end..]);
    if let Some(end) = pattern_end(rest, 0, "only") {
        rest = trim_separators(&rest[end..]);
    }
    let (table, tail) = parse_relation_path_prefix(rest)?;
    if table.is_empty() {
        return Err(SqlError::syntax("MERGE requires a target table".into()));
    }
    let (alias, tail) = super::dml_targets::split_target_alias(tail.trim_start_matches('*'))?;
    let using_end = pattern_end(tail, 0, "using")
        .ok_or_else(|| SqlError::syntax("MERGE requires USING".into()))?;
    let (body, returning, returning_aliases) =
        super::dml_targets::split_returning(&tail[using_end..])?;
    let first_when = find_top_level_keyword(body, 0, "when").ok_or_else(|| {
        SqlError::syntax("syntax error at end of input: MERGE requires a WHEN clause".into())
    })?;
    let joined = &body[..first_when];
    let on_position = last_top_level(joined, "on")
        .ok_or_else(|| SqlError::syntax("MERGE requires an ON condition".into()))?;
    let source = super::dml::parse_mutation_source(&joined[..on_position], "USING")?;
    let on = parse_expression(&joined[on_position + 2..])?;
    let clauses = parse_when_clauses(&body[first_when..])?;
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Merge(Box::new(MergeStatement {
            ctes,
            table,
            alias,
            source,
            on,
            clauses,
            returning,
            returning_aliases,
        })),
    })
}

fn last_top_level(raw: &str, keyword: &str) -> Option<usize> {
    let mut found = None;
    let mut cursor = 0;
    while let Some((position, end)) =
        super::clauses::find_top_level_keyword_span(raw, cursor, keyword)
    {
        found = Some(position);
        cursor = end;
    }
    found
}

fn parse_when_clauses(raw: &str) -> Result<Vec<MergeClause>, SqlError> {
    let mut positions = Vec::new();
    let mut cursor = 0;
    while let Some((position, end)) =
        super::clauses::find_top_level_keyword_span(raw, cursor, "when")
    {
        positions.push((position, end));
        cursor = end;
    }
    let mut clauses = Vec::with_capacity(positions.len());
    for (index, (_, start)) in positions.iter().enumerate() {
        let end = positions
            .get(index + 1)
            .map_or(raw.len(), |(position, _)| *position);
        clauses.push(parse_when_clause(trim_separators(&raw[*start..end]))?);
    }
    Ok(clauses)
}

fn parse_when_clause(raw: &str) -> Result<MergeClause, SqlError> {
    let (kind, rest) = if let Some(end) = pattern_end(raw, 0, "matched") {
        (MergeMatch::Matched, &raw[end..])
    } else if let Some(end) = pattern_end(raw, 0, "not matched") {
        let rest = trim_separators(&raw[end..]);
        if let Some(end) = pattern_end(rest, 0, "by source") {
            (MergeMatch::NotMatchedBySource, &rest[end..])
        } else if let Some(end) = pattern_end(rest, 0, "by target") {
            (MergeMatch::NotMatchedByTarget, &rest[end..])
        } else {
            (MergeMatch::NotMatchedByTarget, rest)
        }
    } else {
        return Err(syntax_near(raw));
    };
    let rest = trim_separators(rest);
    let then = find_top_level_keyword(rest, 0, "then").ok_or_else(|| syntax_near(rest))?;
    let condition_raw = trim_separators(&rest[..then]);
    let condition = if condition_raw.is_empty() {
        None
    } else {
        let end = pattern_end(condition_raw, 0, "and").ok_or_else(|| syntax_near(condition_raw))?;
        Some(parse_expression(&condition_raw[end..])?)
    };
    let action_raw = trim_separators(&rest[then + 4..]);
    let action = parse_merge_action(action_raw)?;
    let allowed = matches!(
        (&kind, &action),
        (_, MergeAction::DoNothing)
            | (
                MergeMatch::Matched | MergeMatch::NotMatchedBySource,
                MergeAction::Update { .. } | MergeAction::Delete,
            )
            | (MergeMatch::NotMatchedByTarget, MergeAction::Insert { .. })
    );
    if !allowed {
        return Err(syntax_near(action_raw));
    }
    Ok(MergeClause {
        kind,
        condition,
        action,
    })
}

fn parse_merge_action(raw: &str) -> Result<MergeAction, SqlError> {
    if let Some(end) = pattern_end(raw, 0, "update set") {
        let parsed = super::dml_targets::parse_set_list(&raw[end..])?;
        return Ok(MergeAction::Update {
            assignments: parsed.assignments,
            row_subqueries: parsed.row_subqueries,
        });
    }
    if raw.eq_ignore_ascii_case("delete") {
        return Ok(MergeAction::Delete);
    }
    if let Some(end) = pattern_end(raw, 0, "do nothing") {
        if trim_separators(&raw[end..]).is_empty() {
            return Ok(MergeAction::DoNothing);
        }
    }
    if let Some(end) = pattern_end(raw, 0, "insert") {
        return parse_merge_insert(trim_separators(&raw[end..]));
    }
    Err(syntax_near(raw))
}

fn parse_merge_insert(raw: &str) -> Result<MergeAction, SqlError> {
    let mut rest = raw;
    let mut columns = Vec::new();
    if rest.starts_with('(') {
        let close = matching_paren(rest, 0).ok_or_else(|| syntax_near(rest))?;
        columns = split_csv(&rest[1..close])
            .into_iter()
            .map(|column| normalize_column_identifier(trim_separators(column)))
            .collect::<Result<Vec<_>, _>>()?;
        rest = trim_separators(&rest[close + 1..]);
    }
    let mut overriding = None;
    if let Some(end) = pattern_end(rest, 0, "overriding system value") {
        overriding = Some(InsertOverriding::System);
        rest = trim_separators(&rest[end..]);
    } else if let Some(end) = pattern_end(rest, 0, "overriding user value") {
        overriding = Some(InsertOverriding::User);
        rest = trim_separators(&rest[end..]);
    }
    if let Some(end) = pattern_end(rest, 0, "default values") {
        if !trim_separators(&rest[end..]).is_empty() || !columns.is_empty() {
            return Err(syntax_near(rest));
        }
        return Ok(MergeAction::Insert {
            columns,
            values: None,
            overriding,
        });
    }
    let end = pattern_end(rest, 0, "values").ok_or_else(|| syntax_near(rest))?;
    let values = trim_separators(&rest[end..]);
    let close = matching_paren(values, 0)
        .filter(|close| values.starts_with('(') && *close + 1 == values.len())
        .ok_or_else(|| syntax_near(values))?;
    let values = split_csv(&values[1..close])
        .into_iter()
        .map(|value| {
            let value = trim_separators(value);
            if value.eq_ignore_ascii_case("default") {
                Ok(super::dml_targets::default_marker())
            } else {
                parse_expression(value)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.is_empty() && columns.len() != values.len() {
        return Err(SqlError::syntax(if values.len() > columns.len() {
            "INSERT has more expressions than target columns".to_string()
        } else {
            "INSERT has more target columns than expressions".to_string()
        }));
    }
    Ok(MergeAction::Insert {
        columns,
        values: Some(values),
        overriding,
    })
}

fn syntax_near(raw: &str) -> SqlError {
    SqlError::syntax(format!(
        "syntax error at or near \"{}\"",
        raw.split_whitespace().next().unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(sql: &str) -> MergeStatement {
        match parse_merge_statement(sql, Vec::new())
            .expect("parse MERGE")
            .statement
        {
            QueryStatement::Merge(statement) => *statement,
            _ => panic!("expected MERGE"),
        }
    }

    #[test]
    fn should_parse_every_merge_clause_kind() {
        // Arrange
        let sql = "MERGE INTO t AS target USING s AS source ON target.id = source.id \
                   WHEN MATCHED AND source.flag = CASE WHEN true THEN 1 ELSE 2 END THEN DELETE \
                   WHEN MATCHED THEN UPDATE SET (a, b) = ROW(source.a, source.b) \
                   WHEN NOT MATCHED BY TARGET THEN INSERT (id, a) VALUES (source.id, DEFAULT) \
                   WHEN NOT MATCHED BY SOURCE THEN DO NOTHING \
                   RETURNING merge_action(), target.id";

        // Act
        let statement = merge(sql);

        // Assert
        assert_eq!(statement.alias.as_deref(), Some("target"));
        assert_eq!(
            statement
                .clauses
                .iter()
                .map(|clause| clause.kind)
                .collect::<Vec<_>>(),
            vec![
                MergeMatch::Matched,
                MergeMatch::Matched,
                MergeMatch::NotMatchedByTarget,
                MergeMatch::NotMatchedBySource,
            ]
        );
        assert!(statement.clauses[0].condition.is_some());
        assert_eq!(statement.returning.len(), 2);
    }

    #[test]
    fn should_take_the_last_on_given_a_joined_merge_source() {
        // Arrange
        let sql = "MERGE INTO t USING a JOIN b ON a.id = b.id ON t.id = a.id \
                   WHEN MATCHED THEN DELETE";

        // Act
        let statement = merge(sql);

        // Assert
        assert!(matches!(
            statement.source.source,
            crate::sql::ast::QuerySource::Join { .. }
        ));
        assert!(statement.alias.is_none());
    }

    #[test]
    fn should_reject_an_insert_action_for_matched_rows() {
        // Arrange
        let sql = "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN INSERT VALUES (1)";

        // Act
        let rejected = parse_merge_statement(sql, Vec::new());

        // Assert
        assert!(rejected.is_err());
    }
}
