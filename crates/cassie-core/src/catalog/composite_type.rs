//! Portable composite type declarations and descriptor helpers.
use serde::{Deserialize, Serialize};
use crate::catalog::scope::{local_name, relation_schema_name};
use crate::types::composite::{CompositeField, CompositeRef, CompositeType};
use crate::types::DataType;

fn normal_oid(object_id: u64) -> Option<u32> {
    if object_id == 0 { return None; }
    u32::try_from(object_id).ok().and_then(|id| id.checked_add(crate::types::object_id::FIRST_NORMAL_OBJECT_ID))
}

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
        normal_oid(self.object_id).unwrap_or(0)
    }

    /// `pg_type.oid` of the matching `_name` array type.
    #[must_use]
    pub fn array_oid(&self) -> u32 {
        normal_oid(self.array_object_id).unwrap_or(0)
    }

    /// `pg_class.oid` of the `relkind = 'c'` row (`pg_type.typrelid`).
    #[must_use]
    pub fn relation_oid(&self) -> u32 {
        normal_oid(self.relation_object_id).unwrap_or(0)
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

