//! Embedded enum descriptors: propagation and startup reconciliation (#827).
//!
//! A table column or view output of an enum type carries a copy of the type's
//! immutable descriptor in its `DataType`. Every alteration that changes the
//! descriptor (`ADD VALUE`, `RENAME VALUE`, rename, schema move) therefore
//! re-publishes each embedding column and view together with the type, in one
//! Midge transaction. Rows are never rewritten: they store member ids.
//!
//! The persisted enum type is the authority. If an interruption ever leaves a
//! column or view on an older copy, hydration replaces it in memory.

use std::collections::HashMap;

use super::composite_alter::ColumnRetype;
use super::{Catalog, ViewMeta};
use crate::types::{DataType, EnumRef};

/// `data_type` with every embedded copy of an enum in `replacements` swapped
/// for the replacement, or `None` when nothing in it changes.
fn rebuild_data_type(
    data_type: &DataType,
    replacements: &HashMap<u32, EnumRef>,
) -> Option<DataType> {
    match data_type {
        DataType::Enum(embedded) => replacements
            .get(&embedded.oid)
            .filter(|replacement| *replacement != embedded)
            .map(|replacement| DataType::Enum(replacement.clone())),
        DataType::Array(inner) => {
            rebuild_data_type(inner, replacements).map(|element| DataType::Array(Box::new(element)))
        }
        _ => None,
    }
}

/// Every object that embeds a changed enum descriptor.
#[derive(Debug, Clone, Default)]
pub struct EnumPropagation {
    pub columns: Vec<ColumnRetype>,
    pub views: Vec<ViewMeta>,
}

impl Catalog {
    /// The columns and views that must be re-published once `target` replaces
    /// its predecessor.
    #[must_use]
    pub fn propagate_enum_change(&self, target: &EnumRef) -> EnumPropagation {
        let replacements = HashMap::from([(target.oid, target.clone())]);
        EnumPropagation {
            columns: self.retyped_enum_columns(&replacements),
            views: self.retyped_enum_views(&replacements),
        }
    }

    fn retyped_enum_columns(&self, replacements: &HashMap<u32, EnumRef>) -> Vec<ColumnRetype> {
        let mut columns = Vec::new();
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if let Some(data_type) = rebuild_data_type(&field.data_type, replacements) {
                    columns.push(ColumnRetype {
                        table: schema.collection.clone(),
                        column: field.name.clone(),
                        data_type,
                    });
                }
            }
        }
        columns
            .sort_by(|left, right| (&left.table, &left.column).cmp(&(&right.table, &right.column)));
        columns
    }

    fn retyped_enum_views(&self, replacements: &HashMap<u32, EnumRef>) -> Vec<ViewMeta> {
        let mut views = Vec::new();
        for mut view in self.list_views() {
            let mut touched = false;
            for field in &mut view.schema.fields {
                if let Some(data_type) = rebuild_data_type(&field.data_type, replacements) {
                    field.data_type = data_type;
                    touched = true;
                }
            }
            if touched {
                views.push(view);
            }
        }
        views
    }

    /// Applies a published propagation to the in-memory catalog.
    pub fn apply_enum_propagation(&self, propagation: EnumPropagation) {
        for column in propagation.columns {
            self.set_collection_field_type(&column.table, &column.column, column.data_type);
        }
        for view in propagation.views {
            self.register_view(view);
        }
    }

    /// Replaces table and view column descriptors that differ from the
    /// registered enum type with the same OID. Returns how many changed.
    #[must_use]
    pub fn reconcile_embedded_enums(&self) -> usize {
        let replacements = self
            .list_enum_types()
            .iter()
            .map(|meta| (meta.type_oid(), meta.descriptor()))
            .collect::<HashMap<_, _>>();
        if replacements.is_empty() {
            return 0;
        }
        let columns = self.retyped_enum_columns(&replacements);
        let views = self.retyped_enum_views(&replacements);
        let changed = columns.len() + views.len();
        self.apply_enum_propagation(EnumPropagation { columns, views });
        changed
    }
}

#[cfg(test)]
#[path = "enum_reconcile_tests.rs"]
mod tests;
