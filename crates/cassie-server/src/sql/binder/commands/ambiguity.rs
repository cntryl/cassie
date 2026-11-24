//! Unqualified columns a joined mutation's target and source both declare.
//!
//! PostgreSQL rejects such a reference with 42702 wherever the target and the
//! source are in scope together: `WHERE`, `ON`, `WHEN` conditions, `SET`
//! values and `RETURNING`. The left side of `SET column = ...` always names a
//! target column and a `MERGE ... INSERT` value list sees only the source, so
//! neither is checked. Subqueries have their own scope and are not walked.

use super::super::{CassieError, CollectionSchema, Expr, HashSet, SelectItem};
use crate::sql::ast::MutationSourceField;
use crate::sql::ColumnIdentifierPath;

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
pub(super) fn check_expr(expr: &Expr, ambiguous: &HashSet<String>) -> Result<(), CassieError> {
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
        Expr::Exists(_) => Ok(()),
        _ => expr.try_visit_children(|child| check_expr(child, ambiguous)),
    }
}

pub(super) fn check_returning(
    items: &[SelectItem],
    ambiguous: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Column { name, .. } => {
                check_expr(&Expr::Column(name.clone()), ambiguous)?;
            }
            SelectItem::Expr { expr, .. } => check_expr(expr, ambiguous)?,
            SelectItem::Function { function, .. } => {
                for arg in &function.args {
                    check_expr(arg, ambiguous)?;
                }
            }
            SelectItem::Wildcard | SelectItem::WindowFunction { .. } => {}
        }
    }
    Ok(())
}
