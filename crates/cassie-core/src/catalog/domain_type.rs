//! Portable domain type declarations and stored-name comparison.
use crate::catalog::scope::{
    local_name, parse_name, relation_database_name, relation_schema_name, ParsedName,
    DEFAULT_SCHEMA,
};
use crate::types::{DataType, DomainCheck, DomainDefault, DomainRef, DomainType};
use serde::{Deserialize, Serialize};

use super::object_identity::normal_oid;

/// The persisted layout version of [`DomainTypeMeta`].
pub const DOMAIN_TYPE_FORMAT_VERSION: u32 = 1;

/// One `CREATE DOMAIN` object, scoped to its database.
///
/// `object_id` and `array_object_id` are drawn from the allocator that also
/// issues table and sequence identities, so they never collide with an object
/// of the same database. They back `pg_type.oid` and `pg_type.typarray`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainTypeMeta {
    pub format_version: u32,
    pub database: String,
    /// Canonical `database.schema.name`.
    pub name: String,
    pub object_id: u64,
    pub array_object_id: u64,
    pub owner_oid: u32,
    pub revision: u64,
    pub base: DataType,
    pub not_null: bool,
    pub default: Option<DomainDefault>,
    pub checks: Vec<DomainCheck>,
}

impl DomainTypeMeta {
    /// The immutable descriptor values and columns of this type carry.
    #[must_use]
    pub fn descriptor(&self) -> DomainRef {
        DomainRef::new(DomainType {
            oid: self.type_oid(),
            array_oid: self.array_oid(),
            qualified_name: self.name.clone(),
            name: local_name(&self.name),
            revision: self.revision,
            base: self.base.clone(),
            not_null: self.not_null,
            default: self.default.clone(),
            checks: self.checks.clone(),
        })
    }

    /// `pg_type.oid` of the domain.
    #[must_use]
    pub fn type_oid(&self) -> u32 {
        normal_oid(self.object_id).unwrap_or(0)
    }

    /// `pg_type.oid` of the matching `_name` array type.
    #[must_use]
    pub fn array_oid(&self) -> u32 {
        normal_oid(self.array_object_id).unwrap_or(0)
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

/// The schema of a relation name stored bare (`public`), schema-qualified or
/// canonical.
fn stored_schema(name: &str) -> String {
    match parse_name(name) {
        Ok(
            ParsedName::SchemaQualified { schema, .. }
            | ParsedName::DatabaseQualified { schema, .. },
        ) => schema,
        _ => DEFAULT_SCHEMA.to_owned(),
    }
}

/// Whether two stored relation or type names denote the same object: equal
/// local names in equal schemas, with the database compared only when both
/// names are canonical. Handles bare, `schema.name` and canonical spellings.
#[must_use]
pub fn same_stored_relation(left: &str, right: &str) -> bool {
    local_name(left) == local_name(right)
        && stored_schema(left) == stored_schema(right)
        && match (relation_database_name(left), relation_database_name(right)) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
}
