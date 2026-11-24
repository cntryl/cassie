//! Portable named enum type declarations.
use serde::{Deserialize, Serialize};
use crate::catalog::scope::{local_name, relation_schema_name};
use crate::types::enum_type::{EnumMember, EnumRef, EnumType};

use super::object_identity::normal_oid;

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
