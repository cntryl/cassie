//! Unqualified columns a joined mutation's target and source both declare.
//!
//! PostgreSQL rejects such a reference with 42702 wherever the target and the
//! source are in scope together: `WHERE`, `ON`, `WHEN` conditions, `SET`
//! values and `RETURNING`. The left side of `SET column = ...` always names a
//! target column and a `MERGE ... INSERT` value list sees only the source, so
//! neither is checked. A subquery keeps that ambiguity check unless its own
//! FROM scope declares the referenced column.

use super::super::{
    CassieError, Catalog, CollectionSchema, Expr, HashSet, ParsedStatement, QuerySource,
    QueryStatement, SelectItem,
};
use crate::sql::ColumnIdentifierPath;
use crate::sql::ast::MutationSourceField;

/// Bare column names declared by both the target and a source relation.
pub(super) fn ambiguous_columns(
    schema: &CollectionSchema,
    source_fields: &[MutationSourceField],
) -> HashSet<String> {
    let target = schema
        .fields
        .iter()
        .map(|field| ColumnIdentifierPath::from_field_name(&field.name).lookup_key())
        .collect::<HashSet<_>>();
    source_fields
        .iter()
        .map(|field| field.column.clone())
        .filter(|column| target.contains(column))
        .collect()
}

/// Rejects an unqualified reference in `expr` that names an ambiguous column.
pub(super) fn check_expr(
    expr: &Expr,
    ambiguous: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    if ambiguous.is_empty() {
        return Ok(());
    }
    match expr {
        Expr::Column(name) => {
            let reference = ColumnIdentifierPath::parse(name).ok();
            match reference {
                Some(reference)
                    if !reference.is_qualified() && ambiguous.contains(&reference.lookup_key()) =>
                {
                    Err(CassieError::AmbiguousColumn(name.clone()))
                }
                _ => Ok(()),
            }
        }
        Expr::Exists(statement) => check_subquery(statement, ambiguous, catalog),
        Expr::Subquery(subquery) => check_subquery(&subquery.query, ambiguous, catalog),
        _ => expr.try_visit_children(|child| check_expr(child, ambiguous, catalog)),
    }
}

pub(super) fn check_returning(
    items: &[SelectItem],
    ambiguous: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Column { name, .. } => {
                check_expr(&Expr::Column(name.clone()), ambiguous, catalog)?;
            }
            SelectItem::Expr { expr, .. } => check_expr(expr, ambiguous, catalog)?,
            SelectItem::Function { function, .. } => {
                for arg in &function.args {
                    check_expr(arg, ambiguous, catalog)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for argument in function
                    .args
                    .iter()
                    .chain(function.filter.as_deref())
                    .chain(&function.partition_by)
                    .chain(function.order_by.iter().map(|order| &order.expr))
                {
                    check_expr(argument, ambiguous, catalog)?;
                }
            }
            SelectItem::Wildcard => {}
        }
    }
    Ok(())
}

fn check_subquery(
    statement: &ParsedStatement,
    ambiguous: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let shadows = select_source_fields(statement);
    let remaining = ambiguous
        .iter()
        .filter(|name| !catalog_source_declares(shadows, name, catalog))
        .cloned()
        .collect();
    let QueryStatement::Select(select) = &statement.statement else {
        return Ok(());
    };
    for item in &select.projection {
        match item {
            SelectItem::Column { name, .. } => {
                check_expr(&Expr::Column(name.clone()), &remaining, catalog)?;
            }
            SelectItem::Expr { expr, .. } => check_expr(expr, &remaining, catalog)?,
            SelectItem::Function { function, .. } => {
                for expression in function.expressions() {
                    check_expr(expression, &remaining, catalog)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for argument in &function.args {
                    check_expr(argument, &remaining, catalog)?;
                }
                if let Some(filter) = function.filter.as_deref() {
                    check_expr(filter, &remaining, catalog)?;
                }
                for partition in &function.partition_by {
                    check_expr(partition, &remaining, catalog)?;
                }
                for order in &function.order_by {
                    check_expr(&order.expr, &remaining, catalog)?;
                }
            }
            SelectItem::Wildcard => {}
        }
    }
    for expression in select
        .distinct_on
        .iter()
        .chain(select.filter.iter())
        .chain(&select.group_by)
        .chain(select.having.iter())
        .chain(select.order.iter().map(|order| &order.expr))
    {
        check_expr(expression, &remaining, catalog)?;
    }
    check_source(&select.source, &remaining, catalog)
}

fn select_source_fields(statement: &ParsedStatement) -> Option<&QuerySource> {
    match &statement.statement {
        QueryStatement::Select(select) => Some(&select.source),
        _ => None,
    }
}

fn check_source(
    source: &QuerySource,
    ambiguous: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Join {
            left, right, on, ..
        } => {
            check_source(left, ambiguous, catalog)?;
            check_source(right, ambiguous, catalog)?;
            check_expr(on, ambiguous, catalog)
        }
        QuerySource::Subquery { select, .. } => check_subquery_parts(select, ambiguous, catalog),
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            check_source(source, ambiguous, catalog)
        }
        _ => Ok(()),
    }
}

fn check_subquery_parts(
    select: &crate::sql::ast::SelectStatement,
    ambiguous: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    for expression in select
        .filter
        .iter()
        .chain(&select.group_by)
        .chain(select.having.iter())
        .chain(select.order.iter().map(|order| &order.expr))
    {
        check_expr(expression, ambiguous, catalog)?;
    }
    check_source(&select.source, ambiguous, catalog)
}

fn catalog_source_declares(source: Option<&QuerySource>, name: &str, catalog: &Catalog) -> bool {
    let Some(source) = source else {
        return false;
    };
    match source {
        QuerySource::Collection(table) => catalog.get_schema(table).is_some_and(|schema| {
            schema.fields.iter().any(|field| {
                ColumnIdentifierPath::from_field_name(&field.name)
                    .lookup_key()
                    .eq_ignore_ascii_case(name)
            })
        }),
        QuerySource::Aliased {
            source,
            column_aliases,
            ..
        } if !column_aliases.is_empty() => column_aliases
            .iter()
            .any(|field| field.eq_ignore_ascii_case(name)),
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            catalog_source_declares(Some(source.as_ref()), name, catalog)
        }
        QuerySource::Join { left, right, .. } => {
            catalog_source_declares(Some(left.as_ref()), name, catalog)
                || catalog_source_declares(Some(right.as_ref()), name, catalog)
        }
        QuerySource::Subquery { select, .. } => select
            .projection
            .iter()
            .any(|item| item.output_name().eq_ignore_ascii_case(name)),
        _ => false,
    }
}
