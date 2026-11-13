//! Relation row types: every table owns a composite type named like the table (#828).
//!
//! The descriptor is derived from the table's current columns, so it stays in
//! step with `ADD`, `RENAME` and `DROP COLUMN` without a second copy. Its
//! identity comes from the table's persisted storage identity, so it survives
//! renames and restarts and needs no extra allocation.
use super::{
    canonical_relation_name, local_name, parse_name, Catalog, CollectionSchema, ParsedName,
};
use crate::types::composite::{CompositeField, CompositeRef, CompositeType};
use crate::types::row_identity::is_row_identity_column;

/// Bit that keeps derived row-type OIDs out of the allocator range.
const DERIVED_OID_FLAG: u32 = 0x8000_0000;

/// A stable `pg_type.oid` derived from a table's `storage_id`; `array` selects
/// the `_name` array type instead.
#[must_use]
pub fn relation_row_type_oid(storage_id: u64, array: bool) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let tag = if array {
        "row-array-type:"
    } else {
        "row-type:"
    };
    for byte in tag.bytes().chain(storage_id.to_be_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let range = u64::from(u32::MAX - DERIVED_OID_FLAG);
    u32::try_from(hash % range).unwrap_or(0) | DERIVED_OID_FLAG
}

fn row_fields(schema: &CollectionSchema) -> Vec<CompositeField> {
    schema
        .fields
        .iter()
        .filter(|field| !is_row_identity_column(&field.name))
        .map(|field| CompositeField {
            name: field.name.clone(),
            data_type: field.data_type.clone(),
        })
        .collect()
}

impl Catalog {
    /// Whether any table exists, i.e. whether a table name can name a type.
    #[must_use]
    pub fn has_collections(&self) -> bool {
        !self.collections.read().is_empty()
    }

    /// The row type of the table with canonical name `canonical`.
    #[must_use]
    pub fn relation_row_type(&self, canonical: &str) -> Option<CompositeRef> {
        let collection = self.get_collection(canonical)?;
        if collection.storage_id == 0 {
            return None;
        }
        let schema = self.get_schema(canonical)?;
        let fields = row_fields(&schema);
        Some(CompositeRef::new(CompositeType {
            oid: relation_row_type_oid(collection.storage_id, false),
            array_oid: relation_row_type_oid(collection.storage_id, true),
            qualified_name: collection.name.clone(),
            name: local_name(&collection.name),
            revision: u64::try_from(fields.len()).unwrap_or(u64::MAX),
            dropped_positions: Vec::new(),
            fields,
        }))
    }

    /// Resolves `raw` as a table name the way a relation name resolves and
    /// returns that table's row type.
    #[must_use]
    pub fn resolve_relation_row_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<CompositeRef> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order.iter().find_map(|schema| {
                self.relation_row_type(&canonical_relation_name(database, schema, &name))
            }),
            ParsedName::SchemaQualified { schema, name } => {
                self.relation_row_type(&canonical_relation_name(database, &schema, &name))
            }
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.relation_row_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }

    /// Row types of every table of `database`, for `pg_type` and `pg_class`.
    #[must_use]
    pub fn list_relation_row_types(
        &self,
        database: Option<&str>,
    ) -> Vec<(String, u64, CompositeRef)> {
        let mut types = Vec::new();
        for collection in self.list_collections() {
            let in_database = database.is_none_or(|database| {
                super::relation_database_name(&collection.name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(database))
            });
            if !in_database {
                continue;
            }
            if let Some(descriptor) = self.relation_row_type(&collection.name) {
                types.push((collection.name.clone(), collection.storage_id, descriptor));
            }
        }
        types.sort_by(|left, right| left.0.cmp(&right.0));
        types
    }
}

#[cfg(test)]
#[path = "relation_row_type_tests.rs"]
mod tests;
