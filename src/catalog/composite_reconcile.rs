//! Startup reconciliation of embedded composite descriptors (#828).
//!
//! `ALTER TYPE` re-embeds the new descriptor in dependent tables and views
//! before it persists the type itself; each write is its own Midge
//! transaction because the objects can live in different databases. A crash
//! between the writes leaves a dependent carrying a descriptor that differs
//! from the persisted type. The persisted type is authoritative: hydration
//! replaces every divergent embedded copy in memory.
use std::collections::HashMap;

use super::Catalog;
use crate::types::composite::CompositeRef;
use crate::types::DataType;

fn reconciled(data_type: &DataType, types: &HashMap<u32, CompositeRef>) -> Option<DataType> {
    match data_type {
        DataType::Composite(embedded) => types
            .get(&embedded.oid)
            .filter(|authoritative| *authoritative != embedded)
            .map(|authoritative| DataType::Composite(authoritative.clone())),
        DataType::Array(inner) => {
            reconciled(inner, types).map(|element| DataType::Array(Box::new(element)))
        }
        _ => None,
    }
}

impl Catalog {
    /// Replaces table and view column descriptors that differ from the
    /// registered composite type with the same OID. Returns how many changed.
    pub fn reconcile_embedded_composites(&self) -> usize {
        let types: HashMap<u32, CompositeRef> = self
            .list_composite_types()
            .iter()
            .map(|meta| (meta.type_oid(), meta.descriptor()))
            .collect();
        if types.is_empty() {
            return 0;
        }
        let mut changes = Vec::new();
        for (table, schema) in self.schemas.read().iter() {
            for field in &schema.fields {
                if let Some(data_type) = reconciled(&field.data_type, &types) {
                    changes.push((table.clone(), field.name.clone(), data_type));
                }
            }
        }
        let count = changes.len();
        for (table, column, data_type) in changes {
            self.set_collection_field_type(&table, &column, data_type);
        }
        let mut healed = 0;
        for mut view in self.list_views() {
            let mut touched = false;
            for field in &mut view.schema.fields {
                if let Some(data_type) = reconciled(&field.data_type, &types) {
                    field.data_type = data_type;
                    touched = true;
                }
            }
            if touched {
                healed += 1;
                self.register_view(view);
            }
        }
        count + healed
    }
}

#[cfg(test)]
#[path = "composite_reconcile_tests.rs"]
mod tests;
