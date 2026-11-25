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

/// `descriptor` with every domain field flattened to its base type. A column
/// that embeds another table's row type stores this form, so the stale domain
/// snapshot never enforces; writes check the live domains instead.
#[must_use]
pub(crate) fn unconstrained(descriptor: &CompositeRef) -> CompositeRef {
    CompositeRef::new(CompositeType {
        fields: descriptor
            .fields
            .iter()
            .map(|field| CompositeField {
                name: field.name.clone(),
                data_type: field.data_type.flattened().clone(),
            })
            .collect(),
        ..(**descriptor).clone()
    })
}

impl Catalog {
    /// The fields of `schema`'s row type. With `constrained`, a domain column
    /// keeps the domain's live descriptor (resolved by OID, as column
    /// enforcement does, so `ALTER DOMAIN` is seen) and casts into the row type
    /// enforce it; otherwise every field is its base type.
    fn row_fields(&self, schema: &CollectionSchema, constrained: bool) -> Vec<CompositeField> {
        schema
            .fields
            .iter()
            .filter(|field| !is_row_identity_column(&field.name))
            .map(|field| CompositeField {
                name: field.name.clone(),
                data_type: match field.domain.as_ref().filter(|_| constrained) {
                    Some(snapshot) => DataType::Domain(
                        self.get_domain_type_by_oid(snapshot.oid)
                            .map_or_else(|| snapshot.clone(), |meta| meta.descriptor()),
                    ),
                    None => field.data_type.clone(),
                },
            })
            .collect()
    }

    /// Whether any table exists, i.e. whether a table name can name a type.
    #[must_use]
    pub fn has_collections(&self) -> bool {
        !self.collections.read().is_empty()
    }

    /// The row type of the table with canonical name `canonical`.
    #[must_use]
    pub fn relation_row_type(&self, canonical: &str) -> Option<CompositeRef> {
        self.relation_row_type_with(canonical, true)
    }

    /// The row type with every domain field flattened to its base type, for
    /// whole-row reads, which never revalidate stored values (as PostgreSQL).
    #[must_use]
    pub fn relation_row_type_unconstrained(&self, canonical: &str) -> Option<CompositeRef> {
        self.relation_row_type_with(canonical, false)
    }

    fn relation_row_type_with(&self, canonical: &str, constrained: bool) -> Option<CompositeRef> {
        let collection = self.get_collection(canonical)?;
        if collection.row_type_object_id == 0 || collection.row_array_object_id == 0 {
            return None;
        }
        let schema = self.get_schema(canonical)?;
        let fields = self.row_fields(&schema, constrained);
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
        // An unscoped catalog stores tables under their bare or schema-qualified
        // name, so a name that is not found canonically is tried exactly as written.
        let exact = |stored: &str| {
            self.get_collection(stored)
                .filter(|collection| collection.name.eq_ignore_ascii_case(stored))
                .and_then(|_| self.relation_row_type(stored))
        };
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order
                .iter()
                .find_map(|schema| {
                    self.relation_row_type(&canonical_relation_name(database, schema, &name))
                })
                .or_else(|| exact(&name)),
            ParsedName::SchemaQualified { schema, name } => self
                .relation_row_type(&canonical_relation_name(database, &schema, &name))
                .or_else(|| exact(&format!("{schema}.{name}"))),
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.relation_row_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }

    /// Records the layout revision of the row type of table `canonical`.
    pub fn set_row_type_revision(&self, canonical: &str, revision: u64) {
        {
            // A catalog hydrated from storage may key the table by a name
            // that only matches `canonical` by scope.
            let mut collections = self.collections.write();
            let key = if collections.contains_key(canonical) {
                Some(canonical.to_owned())
            } else {
                collections
                    .keys()
                    .find(|stored| super::name_matches(stored, canonical))
                    .cloned()
            };
            if let Some(collection) = key.and_then(|key| collections.get_mut(&key)) {
                collection.row_type_revision = revision;
            }
        }
        self.bump_version();
    }

    /// The table whose row type or row array type has `pg_type.oid` `oid`,
    /// with its descriptor and whether `oid` names the array type. The table
    /// is named by its stored (schema-qualifying) name.
    #[must_use]
    pub fn relation_row_type_by_oid(&self, oid: u32) -> Option<(String, CompositeRef, bool)> {
        self.list_collections_canonical()
            .into_iter()
            .find_map(|collection| {
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
        // Canonical names keep each table's own database and schema: two
        // schemas may hold the same table name, and a local name would both
        // resolve to one of them twice and hide which database owns it.
        let scoped = !self.list_databases().is_empty();
        let mut types = Vec::new();
        for collection in self.list_collections_canonical() {
            // Only an instance without a database catalog stores tables under
            // unscoped names; with databases registered such a name has none.
            let in_database = database.is_none_or(|database| {
                super::relation_database_name(&collection.name)
                    .map_or(!scoped, |name| name.eq_ignore_ascii_case(database))
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
