//! Composite type objects: durable identity, name resolution and dependencies (#828).
use serde::{Deserialize, Serialize};

use super::{
    canonical_relation_name, local_name, name_matches, parse_name, relation_database_name,
    relation_schema_name, Catalog, ParsedName,
};
use crate::types::composite::{CompositeField, CompositeRef, CompositeType};
use crate::types::DataType;

/// The persisted layout version of [`CompositeTypeMeta`].
pub const COMPOSITE_TYPE_FORMAT_VERSION: u32 = 1;

/// One `CREATE TYPE ... AS (...)` object, scoped to its database.
///
/// `object_id`, `array_object_id` and `relation_object_id` are drawn from the
/// allocator that also issues table and sequence identities, so they never
/// collide with an object of the same database. They back `pg_type.oid`,
/// `pg_type.typarray` and the `pg_class` row of kind `c` (`typrelid`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeTypeMeta {
    pub format_version: u32,
    pub database: String,
    /// Canonical `database.schema.name`.
    pub name: String,
    pub object_id: u64,
    pub array_object_id: u64,
    pub relation_object_id: u64,
    pub owner_oid: u32,
    pub revision: u64,
    pub fields: Vec<CompositeField>,
    /// Physical slots of dropped attributes (see `CompositeType::dropped_positions`).
    #[serde(default)]
    pub dropped_positions: Vec<usize>,
}

impl CompositeTypeMeta {
    /// The immutable descriptor values of this type carry.
    #[must_use]
    pub fn descriptor(&self) -> CompositeRef {
        CompositeRef::new(CompositeType {
            oid: self.type_oid(),
            array_oid: self.array_oid(),
            qualified_name: self.name.clone(),
            name: local_name(&self.name),
            revision: self.revision,
            dropped_positions: self.dropped_positions.clone(),
            fields: self.fields.clone(),
        })
    }

    /// `pg_type.oid` of the composite type.
    #[must_use]
    pub fn type_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.object_id).unwrap_or(0)
    }

    /// `pg_type.oid` of the matching `_name` array type.
    #[must_use]
    pub fn array_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.array_object_id).unwrap_or(0)
    }

    /// `pg_class.oid` of the `relkind = 'c'` row (`pg_type.typrelid`).
    #[must_use]
    pub fn relation_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.relation_object_id).unwrap_or(0)
    }

    #[must_use]
    pub fn schema_name(&self) -> String {
        relation_schema_name(&self.name)
    }

    #[must_use]
    pub fn local_name(&self) -> String {
        local_name(&self.name)
    }
}

/// Whether `data_type` is, contains or arrays the composite type `oid`.
#[must_use]
pub fn data_type_depends_on(data_type: &DataType, oid: u32) -> bool {
    match data_type {
        DataType::Composite(descriptor) => {
            descriptor.oid == oid
                || descriptor
                    .fields
                    .iter()
                    .any(|field| data_type_depends_on(&field.data_type, oid))
        }
        DataType::Array(inner) => data_type_depends_on(inner, oid),
        _ => false,
    }
}

impl Catalog {
    pub fn register_composite_type(&self, metadata: CompositeTypeMeta) {
        self.composite_types
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_composite_type(&self, name: &str) {
        let mut types = self.composite_types.write();
        let key = name.to_ascii_lowercase();
        if types.remove(&key).is_none() {
            let matching = types
                .iter()
                .find(|(_, metadata)| name_matches(&metadata.name, name))
                .map(|(stored, _)| stored.clone());
            if let Some(stored) = matching {
                types.remove(&stored);
            }
        }
        drop(types);
        self.bump_version();
    }

    #[must_use]
    pub fn get_composite_type(&self, name: &str) -> Option<CompositeTypeMeta> {
        let types = self.composite_types.read();
        types.get(&name.to_ascii_lowercase()).cloned().or_else(|| {
            types
                .values()
                .find(|metadata| name_matches(&metadata.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn has_composite_types(&self) -> bool {
        !self.composite_types.read().is_empty()
    }

    /// A composite type owns a `pg_class` row, so creating a view, sequence,
    /// index or table of the same name is `42P07`, as in PostgreSQL.
    ///
    /// # Errors
    /// Returns `42P07` when a composite type holds `name`.
    pub fn reject_composite_relation_name(
        &self,
        name: &str,
    ) -> Result<(), crate::app::CassieError> {
        if self.composite_type_exists(name) {
            return Err(crate::app::DiagnosticError::new(
                "42P07",
                format!("relation \"{}\" already exists", local_name(name)),
            )
            .into());
        }
        Ok(())
    }

    #[must_use]
    pub fn composite_type_exists(&self, name: &str) -> bool {
        self.get_composite_type(name).is_some()
    }

    /// The type whose `pg_type.oid` is `oid`.
    #[must_use]
    pub fn get_composite_type_by_oid(&self, oid: u32) -> Option<CompositeTypeMeta> {
        self.composite_types
            .read()
            .values()
            .find(|metadata| metadata.type_oid() == oid)
            .cloned()
    }

    /// The type whose array type has `pg_type.oid` `oid`.
    #[must_use]
    pub fn get_composite_type_by_array_oid(&self, oid: u32) -> Option<CompositeTypeMeta> {
        self.composite_types
            .read()
            .values()
            .find(|metadata| metadata.array_oid() == oid)
            .cloned()
    }

    #[must_use]
    pub fn list_composite_types(&self) -> Vec<CompositeTypeMeta> {
        let mut out = self
            .composite_types
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn list_composite_types_for_database(&self, database: &str) -> Vec<CompositeTypeMeta> {
        self.list_composite_types()
            .into_iter()
            .filter(|metadata| metadata.database.eq_ignore_ascii_case(database))
            .collect()
    }

    /// Resolves a type name the way a relation name resolves: an unqualified
    /// name searches `search_order`, a qualified name is exact.
    #[must_use]
    pub fn resolve_composite_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<CompositeTypeMeta> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order.iter().find_map(|schema| {
                self.get_composite_type(&canonical_relation_name(database, schema, &name))
            }),
            ParsedName::SchemaQualified { schema, name } => {
                self.get_composite_type(&canonical_relation_name(database, &schema, &name))
            }
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.get_composite_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }

    /// Objects that keep the type `oid` alive, phrased as PostgreSQL phrases the
    /// `DROP TYPE` dependency detail (`column c of table t`).
    #[must_use]
    pub fn composite_type_dependents(&self, oid: u32) -> Vec<String> {
        self.composite_type_dependents_with_owner(oid)
            .into_iter()
            .map(|(_, phrase)| phrase)
            .collect()
    }

    /// Like [`Self::composite_type_dependents`], pairing each phrase with the
    /// canonical name of the owning table or type.
    #[must_use]
    pub fn composite_type_dependents_with_owner(&self, oid: u32) -> Vec<(String, String)> {
        let mut dependents = Vec::new();
        for metadata in self.composite_types.read().values() {
            if metadata.type_oid() == oid {
                continue;
            }
            for field in &metadata.fields {
                if data_type_depends_on(&field.data_type, oid) {
                    dependents.push((
                        metadata.name.clone(),
                        format!(
                            "column {} of composite type {}",
                            field.name,
                            metadata.local_name()
                        ),
                    ));
                }
            }
        }
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if data_type_depends_on(&field.data_type, oid) {
                    dependents.push((
                        schema.collection.clone(),
                        format!(
                            "column {} of table {}",
                            field.name,
                            local_name(&schema.collection)
                        ),
                    ));
                }
            }
        }
        // A routine taking or returning the type keeps it alive as well.
        dependents.extend(self.routine_dependents_on_type(oid));
        dependents.sort();
        dependents
    }

    /// Removes every type of `database` from the catalog (database drop).
    pub fn unregister_composite_types_for_database(&self, database: &str) {
        self.composite_types
            .write()
            .retain(|_, metadata| !metadata.database.eq_ignore_ascii_case(database));
        self.bump_version();
    }

    /// Types declared in `schema` of `database`, for `DROP SCHEMA` dependency checks.
    #[must_use]
    pub fn composite_types_in_schema(
        &self,
        database: &str,
        schema: &str,
    ) -> Vec<CompositeTypeMeta> {
        self.list_composite_types()
            .into_iter()
            .filter(|metadata| {
                relation_database_name(&metadata.name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(database))
                    && metadata.schema_name().eq_ignore_ascii_case(schema)
            })
            .collect()
    }
}
