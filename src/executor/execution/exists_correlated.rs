//! Correlated `EXISTS` in a `WHERE` filter.
//!
//! A subquery that references the enclosing row cannot be folded to one
//! boolean up front. Each candidate row is instead offered to the subquery as
//! its outer row (the same mechanism a `LATERAL` source uses): the binder sees
//! the row's columns as extra known fields and the executor merges the row
//! into every subquery source row, so `orders.uid = users.uid` compares the
//! inner column with this outer row's value.

use super::{
    batch, filter, resolve_exists_expr, Batch, BatchRow, ExistsResolutionContext, Expr, QueryError,
    QuerySource,
};

use super::{Cassie, HashSet, LogicalPlan};

/// Returns whether `expr` contains an `EXISTS` subquery.
pub(super) fn contains_exists(expr: &Expr) -> bool {
    expr.any_descendant_or_self(&mut |expr| matches!(expr, Expr::Exists(_)))
}

/// Keeps the rows of `batches` for which `filter_expr` holds, evaluating each
/// `EXISTS` with that row as the subquery's outer row.
///
/// # Errors
///
/// Returns an error when a subquery fails to bind or execute.
pub(super) fn filter_rows_per_outer_row(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    filter_expr: &Expr,
    batches: Vec<Batch>,
    search_context: Option<&filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let qualifier = outer_qualifier(source);
    let mut kept = Vec::new();
    for row in batch::flatten_batches(batches) {
        super::check_timeout(context.controls)?;
        let outer = qualified_outer_row(&row, qualifier.as_deref());
        let row_context = ExistsResolutionContext {
            outer_row: Some(&outer),
            ..*context
        };
        let resolved = resolve_exists_expr(&row_context, filter_expr)?;
        kept.extend(filter::filter_rows(
            vec![row],
            &resolved,
            context.params,
            search_context,
            context.user_functions,
            context.session,
        )?);
    }
    Ok(batch::chunk_rows(kept, batch::DEFAULT_BATCH_SIZE))
}

/// The name the enclosing relation's columns are qualified with, for a
/// single-relation source whose rows carry unqualified column names.
fn outer_qualifier(source: &QuerySource) -> Option<String> {
    match source {
        QuerySource::Collection(name) => {
            Some(crate::catalog::local_name(name).to_ascii_lowercase())
        }
        QuerySource::Subquery { alias, .. } => Some(alias.to_ascii_lowercase()),
        QuerySource::Aliased { alias, .. } => Some(crate::sql::binder::alias_row_qualifier(alias)),
        _ => None,
    }
}

/// Renames every unqualified entry to `qualifier.column`, so an unqualified
/// name inside the subquery resolves to the subquery's own columns first
/// (PostgreSQL scoping) and reaches the outer row only through its suffix.
fn qualified_outer_row(row: &BatchRow, qualifier: Option<&str>) -> BatchRow {
    let Some(qualifier) = qualifier else {
        return row.clone();
    };
    BatchRow::new(
        row.entries()
            .iter()
            .map(|(name, value)| {
                if name.contains('.') {
                    (name.clone(), value.clone())
                } else {
                    (format!("{qualifier}.{name}"), value.clone())
                }
            })
            .collect(),
    )
}

/// Returns the qualified outer row with unqualified aliases for the columns
/// the subquery's own relations do not define, so an unqualified name
/// resolves to the innermost scope first, as in PostgreSQL.
pub(super) fn scoped_outer_row(
    cassie: &Cassie,
    subquery: &LogicalPlan,
    row: &BatchRow,
) -> BatchRow {
    let mut inner_columns = HashSet::new();
    collect_source_columns(cassie, &subquery.source, &mut inner_columns);
    let aliases = row
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(index, (name, _))| {
            let column = crate::sql::ColumnIdentifierPath::stored_row_field_key(name);
            (!inner_columns.contains(&column)).then_some((column, index))
        })
        .collect();
    BatchRow::with_aliases(row.entries().to_vec(), aliases)
}

fn collect_source_columns(cassie: &Cassie, source: &QuerySource, columns: &mut HashSet<String>) {
    match source {
        QuerySource::Collection(name) => {
            if let Some(schema) = cassie.catalog.get_schema(name) {
                columns.extend(
                    schema.fields.iter().map(|field| {
                        crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
                    }),
                );
            }
        }
        QuerySource::Aliased { source, .. } => collect_source_columns(cassie, source, columns),
        QuerySource::Join { left, right, .. } => {
            collect_source_columns(cassie, left, columns);
            collect_source_columns(cassie, right, columns);
        }
        _ => {}
    }
}
