//! Preserve declared ARRAY and CHAR semantics through a projection without
//! changing values, aliases or equality keys.
use std::collections::HashMap;
use std::sync::Arc;

use crate::catalog::FunctionMeta;
use crate::types::DataType;

use super::array_order;
use super::batch::RowAccess;
use super::projection::ProjectionOp;

pub(super) fn projected<R: RowAccess>(
    row: &R,
    ops: &[ProjectionOp],
    functions: &HashMap<String, FunctionMeta>,
) -> Option<Arc<Vec<DataType>>> {
    if !required(row) {
        return None;
    }
    let mut types = Vec::new();
    for op in ops {
        match op {
            ProjectionOp::Wildcard => {
                let hide_identity = row
                    .entries()
                    .iter()
                    .any(|(name, _)| crate::types::row_identity::is_legacy_id_column(name));
                types.extend(
                    row.entries()
                        .iter()
                        .enumerate()
                        .filter(|(_, (name, _))| {
                            !super::projection::is_window_value_key(name)
                                && (!hide_identity
                                    || !crate::types::row_identity::is_row_identity_column(name))
                        })
                        .map(|(index, _)| row.entry_type(index).cloned().unwrap_or(DataType::Null)),
                );
            }
            ProjectionOp::Column { source, .. } | ProjectionOp::WindowFunction { source, .. } => {
                types.push(row.column_type(source).cloned().unwrap_or(DataType::Null));
            }
            ProjectionOp::AggregateFunction { lookups, .. } => types.push(
                lookups
                    .iter()
                    .find_map(|name| row.column_type(name))
                    .cloned()
                    .unwrap_or(DataType::Null),
            ),
            ProjectionOp::ScalarFunction {
                expr,
                precomputed_key,
                ..
            } => types.push(
                precomputed_key
                    .as_ref()
                    .and_then(|name| row.column_type(name))
                    .cloned()
                    .or_else(|| array_order::expression_type(row, expr, functions))
                    .unwrap_or(DataType::Null),
            ),
        }
    }
    types
        .iter()
        .any(|data_type| matches!(data_type, DataType::Array(_) | DataType::Char { .. }))
        .then(|| types.into())
}

/// Both array ordering and bpchar equality require declared column identity.
pub(super) fn required(row: &impl RowAccess) -> bool {
    row.has_array_types()
        || row
            .entries()
            .iter()
            .enumerate()
            .any(|(index, _)| matches!(row.entry_type(index), Some(DataType::Char { .. })))
}
