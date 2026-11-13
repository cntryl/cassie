//! Named enum type objects: durable identity, name resolution and lookup (#827).
//!
//! The design authority is the "Selected v0.4.0 named enum contract" section of
//! `docs/type-contract.md`.
use serde::{Deserialize, Serialize};

use super::{
    canonical_relation_name, local_name, name_matches, parse_name, relation_schema_name, Catalog,
    ParsedName,
};
use crate::types::enum_type::{EnumMember, EnumRef, EnumType};
use crate::types::DataType;

/// The persisted layout version of [`EnumTypeMeta`].
pub const ENUM_TYPE_FORMAT_VERSION: u32 = 1;

/// One `CREATE TYPE ... AS ENUM` object, scoped to its database.
///
/// `object_id`, `array_object_id` and every `members[].member_id` come from
/// the persistent object allocator, which commits independently of the DDL
/// overlay: an identity allocated by a rolled-back statement is burned and
/// never reused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumTypeMeta {
    pub format_version: u32,
    pub database: String,
    /// Canonical `database.schema.name`.
    pub name: String,
    pub object_id: u64,
    pub array_object_id: u64,
    pub owner_oid: u32,
    pub revision: u64,
    /// Members in sort order.
    pub members: Vec<EnumMember>,
}

impl EnumTypeMeta {
    /// The immutable descriptor values of this type carry.
    #[must_use]
    pub fn descriptor(&self) -> EnumRef {
        EnumRef::new(EnumType {
            oid: self.type_oid(),
            array_oid: self.array_oid(),
            qualified_name: self.name.clone(),
            name: local_name(&self.name),
            revision: self.revision,
            members: self.members.clone(),
        })
    }

    /// `pg_type.oid` of the enum type.
    #[must_use]
    pub fn type_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.object_id).unwrap_or(0)
    }

    /// `pg_type.oid` of the matching `_name` array type.
    #[must_use]
    pub fn array_oid(&self) -> u32 {
        super::object_identity::normal_oid(self.array_object_id).unwrap_or(0)
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

/// Whether `data_type` is, or arrays, the enum type `oid`.
#[must_use]
pub fn data_type_depends_on_enum(data_type: &DataType, oid: u32) -> bool {
    match data_type {
        DataType::Enum(descriptor) => descriptor.oid == oid,
        DataType::Array(inner) => data_type_depends_on_enum(inner, oid),
        _ => false,
    }
}

impl Catalog {
    pub fn register_enum_type(&self, metadata: EnumTypeMeta) {
        self.enum_types
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_enum_type(&self, name: &str) {
        let mut types = self.enum_types.write();
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
    pub fn get_enum_type(&self, name: &str) -> Option<EnumTypeMeta> {
        let types = self.enum_types.read();
        types.get(&name.to_ascii_lowercase()).cloned().or_else(|| {
            types
                .values()
                .find(|metadata| name_matches(&metadata.name, name))
                .cloned()
        })
    }

    #[must_use]
    pub fn has_enum_types(&self) -> bool {
        !self.enum_types.read().is_empty()
    }

    #[must_use]
    pub fn enum_type_exists(&self, name: &str) -> bool {
        self.get_enum_type(name).is_some()
    }

    /// The type whose `pg_type.oid` is `oid`.
    #[must_use]
    pub fn get_enum_type_by_oid(&self, oid: u32) -> Option<EnumTypeMeta> {
        self.enum_types
            .read()
            .values()
            .find(|metadata| metadata.type_oid() == oid)
            .cloned()
    }

    /// The type whose array type has `pg_type.oid` `oid`.
    #[must_use]
    pub fn get_enum_type_by_array_oid(&self, oid: u32) -> Option<EnumTypeMeta> {
        self.enum_types
            .read()
            .values()
            .find(|metadata| metadata.array_oid() == oid)
            .cloned()
    }

    #[must_use]
    pub fn list_enum_types(&self) -> Vec<EnumTypeMeta> {
        let mut out = self.enum_types.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        out
    }

    #[must_use]
    pub fn list_enum_types_for_database(&self, database: &str) -> Vec<EnumTypeMeta> {
        self.list_enum_types()
            .into_iter()
            .filter(|metadata| metadata.database.eq_ignore_ascii_case(database))
            .collect()
    }

    /// Resolves a type name the way a relation name resolves: an unqualified
    /// name searches `search_order`, a qualified name is exact.
    #[must_use]
    pub fn resolve_enum_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<EnumTypeMeta> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => search_order.iter().find_map(|schema| {
                self.get_enum_type(&canonical_relation_name(database, schema, &name))
            }),
            ParsedName::SchemaQualified { schema, name } => {
                self.get_enum_type(&canonical_relation_name(database, &schema, &name))
            }
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.get_enum_type(&canonical_relation_name(&database, &schema, &name)),
        }
    }
}
