//! The plain scalar index a graph read uses to find a mapped row by its key columns.
//!
//! Shared by the `GRAPH_TABLE` lazy store and the `cassie_graph` reference constructors
//! so both decide index usability the same way.

use super::{Catalog, IndexKind, IndexMeta};
use crate::app::CassieSession;

/// The scalar index of `table` on exactly `columns`, in any column order, usable by the
/// session's snapshot, with, for each index field, the position of its column in `columns`.
/// Partial and expression indexes never qualify, and neither does an index with a nullable
/// suffix: those rows may be absent from the index entirely.
#[must_use]
pub fn scalar_key_index(
    catalog: &Catalog,
    session: Option<&CassieSession>,
    table: &str,
    columns: &[String],
) -> Option<(IndexMeta, Vec<usize>)> {
    catalog.list_indexes(table).into_iter().find_map(|index| {
        if index.kind != IndexKind::Scalar
            || index.predicate.is_some()
            || !index.expressions.is_empty()
        {
            return None;
        }
        let fields = index.normalized_fields();
        if fields.len() < columns.len() {
            return None;
        }
        let (prefix, suffix) = fields.split_at(columns.len());
        if !suffix.is_empty() {
            let constraints = catalog.get_constraints(table);
            if !suffix.iter().all(|field| {
                constraints.iter().any(|constraint| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(field, &constraint.field)
                        && (constraint.not_null || constraint.primary_key)
                })
            }) {
                return None;
            }
        }
        let order = prefix
            .iter()
            .map(|field| {
                columns.iter().position(|column| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(field, column)
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut seen = order.clone();
        seen.sort_unstable();
        seen.dedup();
        if seen.len() != columns.len() {
            return None;
        }
        if session.is_some_and(|session| !session.transaction_index_visible(&index, catalog)) {
            return None;
        }
        Some((index, order))
    })
}
