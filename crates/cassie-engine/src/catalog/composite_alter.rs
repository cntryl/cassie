//! Descriptor transformations behind `ALTER TYPE` (#828).
//!
//! A composite descriptor is immutable. Every change publishes a new revision
//! built from the previous one and then re-embeds the result in every object
//! that carries a copy: other composite types, table columns and views.
//!
//! The physical slot list only grows. Adding an attribute appends a slot,
//! dropping one records its slot in `dropped_positions`, so a value written
//! under any earlier revision is a prefix of the current layout.
pub use cassie_sql::type_publication::{ColumnRetype, CompositeGroup};

use std::collections::HashMap;

use super::composite_types::{data_type_depends_on, CompositeTypeMeta};
use super::{local_name, Catalog};
use crate::types::composite::CompositeRef;
use crate::types::DataType;

pub use cassie_core::catalog::composite_type::TypeAlterError;

/// `data_type` with every embedded composite replaced from `replacements`, or
/// `None` when nothing in it changes.
fn rebuild_data_type(
    data_type: &DataType,
    replacements: &HashMap<u32, CompositeRef>,
) -> Option<DataType> {
    match data_type {
        DataType::Composite(descriptor) => replacements
            .get(&descriptor.oid)
            .map(|replacement| DataType::Composite(replacement.clone())),
        DataType::Array(inner) => {
            rebuild_data_type(inner, replacements).map(|element| DataType::Array(Box::new(element)))
        }
        _ => None,
    }
}

/// Every object that embeds a changed descriptor.
#[derive(Debug, Clone, Default)]
pub struct DescriptorPropagation {
    /// Other composite types whose fields embed the changed type.
    pub types: Vec<CompositeTypeMeta>,
    pub columns: Vec<ColumnRetype>,
    pub views: Vec<crate::catalog::ViewMeta>,
}

impl Catalog {
    /// Table columns, as `table.column`, that store `oid` directly or inside
    /// another composite type. PostgreSQL refuses a retype while any exists.
    #[must_use]
    pub fn composite_type_stored_in(&self, oid: u32) -> Vec<String> {
        let mut columns = Vec::new();
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if data_type_depends_on(&field.data_type, oid) {
                    columns.push(format!("{}.{}", local_name(&schema.collection), field.name));
                }
            }
        }
        columns.sort();
        columns
    }

    /// The objects that must be updated once `target` replaces its predecessor.
    #[must_use]
    pub fn propagate_composite_change(&self, target: &CompositeRef) -> DescriptorPropagation {
        let originals = self.list_composite_types();
        let mut replacements: HashMap<u32, CompositeRef> =
            HashMap::from([(target.oid, target.clone())]);
        let mut rebuilt: HashMap<u32, CompositeTypeMeta> = HashMap::new();
        loop {
            let mut changed = false;
            for original in &originals {
                if original.type_oid() == target.oid {
                    continue;
                }
                let mut next = original.clone();
                let mut touched = false;
                for field in &mut next.fields {
                    if let Some(data_type) = rebuild_data_type(&field.data_type, &replacements) {
                        field.data_type = data_type;
                        touched = true;
                    }
                }
                if !touched {
                    continue;
                }
                next.revision = original.revision + 1;
                let descriptor = next.descriptor();
                if replacements.get(&next.type_oid()) != Some(&descriptor) {
                    replacements.insert(next.type_oid(), descriptor);
                    rebuilt.insert(next.type_oid(), next);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut types = rebuilt.into_values().collect::<Vec<_>>();
        types.sort_by_key(|meta| meta.name.to_ascii_lowercase());
        DescriptorPropagation {
            types,
            columns: self.retyped_columns(&replacements),
            views: self.retyped_views(&replacements),
        }
    }

    fn retyped_columns(&self, replacements: &HashMap<u32, CompositeRef>) -> Vec<ColumnRetype> {
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

    fn retyped_views(
        &self,
        replacements: &HashMap<u32, CompositeRef>,
    ) -> Vec<crate::catalog::ViewMeta> {
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
}

#[cfg(test)]
#[path = "composite_alter_tests.rs"]
mod tests;
