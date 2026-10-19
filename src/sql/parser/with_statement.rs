//! `WITH ... INSERT | UPDATE | DELETE`: the statement keeps its `WITH` list,
//! and every query nested in it also receives the definitions, so a
//! reference resolves to the CTE wherever the statement reads it.
use super::clauses::find_top_level_keyword;
use super::lexical::{matching_paren, pattern_end, separator_end, trim_separators};
use super::SqlError;
use super::{parse_statement, CommonTableExpression, Expr, ParsedStatement, QueryStatement};
use crate::sql::ast::{InsertSource, SelectItem, SelectStatement};

/// Splits a `WITH` list (after `WITH [RECURSIVE]`) from a data-modifying
/// statement that follows it, or `None` when another statement follows.
pub(super) fn split_dml(sql: &str) -> Option<(&str, &str)> {
    let mut cursor = 0;
    loop {
        let as_pos = find_top_level_keyword(sql, cursor, "as")?;
        let mut position = separator_end(sql, as_pos + 2);
        for keyword in ["not", "materialized"] {
            if let Some(end) = pattern_end(sql, position, keyword) {
                position = separator_end(sql, end);
            }
        }
        let close = matching_paren(sql, position)?;
        position = separator_end(sql, close + 1);
        if sql[position..].starts_with(',') {
            cursor = position + 1;
            continue;
        }
        let main = &sql[position..];
        return ["insert", "update", "delete"]
            .iter()
            .any(|keyword| pattern_end(main, 0, keyword).is_some())
            .then(|| (trim_separators(&sql[..position]), main));
    }
}

pub(super) fn parse(
    ctes: Vec<CommonTableExpression>,
    main: &str,
) -> Result<ParsedStatement, SqlError> {
    let mut parsed = parse_statement(main)?;
    match &mut parsed.statement {
        QueryStatement::Insert(insert) => {
            if let InsertSource::Select(select) = &mut insert.source {
                prepend(select, &ctes);
            }
            if let InsertSource::Values(rows) = &mut insert.source {
                for expr in rows.iter_mut().flatten() {
                    nested(expr, &ctes);
                }
            }
            items(&mut insert.returning, &ctes);
            insert.ctes = ctes;
        }
        QueryStatement::Update(update) => {
            for (_, expr) in &mut update.assignments {
                nested(expr, &ctes);
            }
            if let Some(expr) = &mut update.filter {
                nested(expr, &ctes);
            }
            for assignment in &mut update.row_subqueries {
                statement(&mut assignment.query, &ctes);
            }
            if let Some(from) = &mut update.from {
                if let Some(select) = &mut from.query {
                    prepend(select, &ctes);
                }
            }
            items(&mut update.returning, &ctes);
            update.ctes = ctes;
        }
        QueryStatement::Delete(delete) => {
            if let Some(expr) = &mut delete.filter {
                nested(expr, &ctes);
            }
            if let Some(using) = &mut delete.using {
                if let Some(select) = &mut using.query {
                    prepend(select, &ctes);
                }
            }
            items(&mut delete.returning, &ctes);
            delete.ctes = ctes;
        }
        _ => {
            return Err(SqlError::syntax(
                "WITH must be followed by SELECT, INSERT, UPDATE, DELETE or MERGE".into(),
            ))
        }
    }
    Ok(parsed)
}

fn items(items: &mut [SelectItem], ctes: &[CommonTableExpression]) {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => nested(expr, ctes),
            SelectItem::Function { function, .. } => {
                for arg in function.expressions_mut() {
                    nested(arg, ctes);
                }
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::WindowFunction { .. } => {}
        }
    }
}

fn nested(expr: &mut Expr, ctes: &[CommonTableExpression]) {
    if let Some(query) = expr.subquery_statement_mut() {
        statement(query, ctes);
    }
    let _ = expr.try_visit_children_mut(|child| {
        nested(child, ctes);
        Ok::<_, ()>(())
    });
}

fn statement(query: &mut ParsedStatement, ctes: &[CommonTableExpression]) {
    if let QueryStatement::Select(select) = &mut query.statement {
        prepend(select, ctes);
    }
}

/// An inner definition of the same name shadows the outer one.
fn prepend(select: &mut SelectStatement, ctes: &[CommonTableExpression]) {
    let inherited = ctes
        .iter()
        .filter(|cte| {
            !select
                .ctes
                .iter()
                .any(|inner| inner.name.eq_ignore_ascii_case(&cte.name))
        })
        .cloned()
        .collect::<Vec<_>>();
    select.recursive |= inherited
        .iter()
        .any(|cte| matches!(cte.query, crate::sql::ast::CteQuery::Recursive { .. }));
    select.ctes.splice(0..0, inherited);
}
