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
use crate::types::DataType;

fn row_fields(schema: &CollectionSchema) -> Vec<CompositeField> {
    schema
        .fields
        .iter()
        .filter(|field| !is_row_identity_column(&field.name))
        .map(|field| CompositeField {
            name: field.name.clone(),
            // A domain column keeps its domain so casts into the row type
            // enforce the constraints, as PostgreSQL's coercion does.
            data_type: field.domain.as_ref().map_or_else(
                || field.data_type.clone(),
                |domain| DataType::Domain(domain.clone()),
            ),
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
        if collection.row_type_object_id == 0 || collection.row_array_object_id == 0 {
            return None;
        }
        let schema = self.get_schema(canonical)?;
        let fields = row_fields(&schema);
        Some(CompositeRef::new(CompositeType {
            oid: super::object_identity::normal_oid(collection.row_type_object_id)?,
            array_oid: super::object_identity::normal_oid(collection.row_array_object_id)?,
            qualified_name: collection.name.clone(),
            name: local_name(&collection.name),
            revision: collection.row_type_revision,
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

    /// Records the layout revision of the row type of table `canonical`.
    pub fn set_row_type_revision(&self, canonical: &str, revision: u64) {
        if let Some(collection) = self.collections.write().get_mut(canonical) {
            collection.row_type_revision = revision;
        }
        self.bump_version();
    }

    /// The table whose row type or row array type has `pg_type.oid` `oid`,
    /// with its descriptor and whether `oid` names the array type.
    #[must_use]
    pub fn relation_row_type_by_oid(&self, oid: u32) -> Option<(String, CompositeRef, bool)> {
        self.list_collections().into_iter().find_map(|collection| {
            let descriptor = self.relation_row_type(&collection.name)?;
            if descriptor.oid == oid {
                Some((collection.name, descriptor, false))
            } else if descriptor.array_oid == oid {
                Some((collection.name, descriptor, true))
            } else {
                None
            }
        })
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
