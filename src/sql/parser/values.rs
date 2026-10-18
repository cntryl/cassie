//! A flat VALUES query source, with the ordinary SELECT ordering/pagination tail.

use super::{parse_select_clauses, resolve_order_ordinals};
use crate::sql::ast::{
    CommonTableExpression, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
use crate::sql::parser::{
    clauses::parse_clauses,
    dml::parse_values_rows,
    expr::{parse_expression, parse_order_by},
    lexical, SqlError,
};

pub(super) fn parse(
    sql: &str,
    ctes: Vec<CommonTableExpression>,
    recursive: bool,
) -> Result<ParsedStatement, SqlError> {
    let end = lexical::pattern_end(sql, 0, "values").expect("VALUES query");
    let body = lexical::trim_separators(&sql[end..]);
    let clauses = parse_clauses(body)?;
    let rows_end = clauses.first().map_or(body.len(), |clause| clause.position);
    let rows = parse_values_rows(body[..rows_end].trim(), parse_expression)?;
    let width = rows[0].len();
    if rows.iter().any(|row| row.len() != width) {
        return Err(SqlError::syntax(
            "VALUES lists must all be the same length".into(),
        ));
    }
    let parsed = parse_select_clauses(body, &clauses)?;
    if parsed.where_sql.is_some() || parsed.group_sql.is_some() || parsed.having_sql.is_some() {
        return Err(SqlError::syntax("unexpected clause after VALUES".into()));
    }
    let projection = (0..width)
        .map(|index| SelectItem::Column {
            name: format!("column{}", index + 1),
            alias: None,
        })
        .collect::<Vec<_>>();
    let order = parsed
        .order_sql
        .as_deref()
        .map(parse_order_by)
        .transpose()?
        .unwrap_or_default();
    let order = resolve_order_ordinals(order, &projection)?;
    Ok(ParsedStatement {
        raw_sql: sql.to_owned(),
        statement: QueryStatement::Select(Box::new(SelectStatement {
            source: QuerySource::Values { rows },
            ctes,
            recursive,
            distinct: false,
            distinct_on: Vec::new(),
            projection,
            filter: None,
            group_by: Vec::new(),
            having: None,
            order,
            limit: parsed.limit,
            offset: parsed.offset,
            set: None,
        })),
    })
}
