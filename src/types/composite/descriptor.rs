//! Immutable versioned descriptors for composite and transient record types.
use std::ops::Deref;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::DataType;

/// The `record` pseudo-type used by anonymous `ROW(...)` values.
pub const RECORD_OID: u32 = 2249;
/// The `_record` pseudo-type.
pub const RECORD_ARRAY_OID: u32 = 2287;
/// The JSON object key that marks the storage form of a record value.
pub const RECORD_STORAGE_KEY: &str = "$cassie_record_v1";

/// One field of a composite type, in declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeField {
    pub name: String,
    pub data_type: DataType,
}

/// A composite type descriptor.
///
/// `revision` identifies the field layout that rows were written with. A
/// descriptor is never mutated in place; changing the layout publishes a new
/// descriptor with a larger revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeType {
    /// Allocated database-scoped type OID, or [`RECORD_OID`] when anonymous.
    pub oid: u32,
    /// Allocated OID of the matching array type (`_name`), or [`RECORD_ARRAY_OID`].
    pub array_oid: u32,
    /// Canonical `database.schema.name`; empty for an anonymous record.
    pub qualified_name: String,
    /// Schema-local type name (`record` when anonymous).
    pub name: String,
    pub revision: u64,
    /// The live (visible) fields in declaration order.
    pub fields: Vec<CompositeField>,
    /// Physical slot indexes of dropped attributes, ascending. A stored value
    /// keeps one slot per attribute ever added, so a dropped attribute keeps
    /// its position and only its (ignored) bytes remain in old rows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped_positions: Vec<usize>,
}

impl CompositeType {
    /// The transient descriptor of a `ROW(...)` value with `field_types` columns
    /// named `f1`, `f2`, ... as PostgreSQL names them.
    #[must_use]
    pub fn anonymous(field_types: Vec<DataType>) -> Self {
        Self {
            oid: RECORD_OID,
            array_oid: RECORD_ARRAY_OID,
            qualified_name: String::new(),
            name: "record".to_string(),
            revision: 0,
            dropped_positions: Vec::new(),
            fields: field_types
                .into_iter()
                .enumerate()
                .map(|(index, data_type)| CompositeField {
                    name: format!("f{}", index + 1),
                    data_type,
                })
                .collect(),
        }
    }

    #[must_use]
    pub const fn is_anonymous(&self) -> bool {
        self.oid == RECORD_OID
    }

    /// Number of physical slots a value written at this revision carries:
    /// live fields plus dropped attribute positions.
    #[must_use]
    pub fn physical_len(&self) -> usize {
        self.fields.len() + self.dropped_positions.len()
    }

    /// The physical slot of every live field, in declaration order.
    #[must_use]
    pub fn physical_positions(&self) -> Vec<usize> {
        (0..self.physical_len())
            .filter(|slot| !self.dropped_positions.contains(slot))
            .collect()
    }

    /// The zero-based position of the field called `name`.
    #[must_use]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }
}

/// A shared, serializable handle to an immutable descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositeRef(Arc<CompositeType>);

impl CompositeRef {
    #[must_use]
    pub fn new(descriptor: CompositeType) -> Self {
        Self(Arc::new(descriptor))
    }
}

impl Deref for CompositeRef {
    type Target = CompositeType;

    fn deref(&self) -> &CompositeType {
        &self.0
    }
}

impl From<CompositeType> for CompositeRef {
    fn from(descriptor: CompositeType) -> Self {
        Self::new(descriptor)
    }
}

impl Serialize for CompositeRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CompositeRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CompositeType::deserialize(deserializer).map(Self::new)
    }
}
