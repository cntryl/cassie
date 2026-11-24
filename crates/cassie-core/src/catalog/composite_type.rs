//! Portable composite type declarations and descriptor helpers.
use serde::{Deserialize, Serialize};
use crate::catalog::scope::{local_name, relation_schema_name};
use crate::types::composite::{CompositeField, CompositeRef, CompositeType};
use crate::types::DataType;

use super::object_identity::normal_oid;

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

/// A rejected `ALTER TYPE` transition with its PostgreSQL error class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAlterError {
    pub sqlstate: &'static str,
    pub message: String,
}

impl TypeAlterError {
    fn new(sqlstate: &'static str, message: String) -> Self {
        Self { sqlstate, message }
    }
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

    fn attribute_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }

    fn require_attribute(&self, name: &str) -> Result<usize, TypeAlterError> {
        self.attribute_index(name).ok_or_else(|| {
            TypeAlterError::new(
                "42703",
                format!("column \"{name}\" of relation \"{}\" does not exist", self.local_name()),
            )
        })
    }

    fn reject_duplicate(&self, name: &str) -> Result<(), TypeAlterError> {
        if self.fields.iter().any(|field| field.name == name) {
            return Err(TypeAlterError::new(
                "42701",
                format!("column \"{name}\" of relation \"{}\" already exists", self.local_name()),
            ));
        }
        Ok(())
    }

    /// # Errors
    /// `42701` when the attribute exists, `42P16` when the type would contain itself.
    pub fn with_added_attribute(
        &self,
        name: &str,
        data_type: DataType,
    ) -> Result<Self, TypeAlterError> {
        self.reject_duplicate(name)?;
        self.reject_self_reference(&data_type)?;
        let mut next = self.clone();
        next.fields.push(CompositeField { name: name.to_owned(), data_type });
        next.revision += 1;
        Ok(next)
    }

    /// # Errors
    /// `42703` when the attribute does not exist.
    pub fn with_dropped_attribute(&self, name: &str) -> Result<Self, TypeAlterError> {
        let index = self.require_attribute(name)?;
        let slot = self.descriptor().physical_positions()[index];
        let mut next = self.clone();
        next.fields.remove(index);
        next.dropped_positions.push(slot);
        next.dropped_positions.sort_unstable();
        next.revision += 1;
        Ok(next)
    }

    /// # Errors
    /// `42703` when `from` is missing, `42701` when `to` exists.
    pub fn with_renamed_attribute(&self, from: &str, to: &str) -> Result<Self, TypeAlterError> {
        let index = self.require_attribute(from)?;
        self.reject_duplicate(to)?;
        let mut next = self.clone();
        to.clone_into(&mut next.fields[index].name);
        next.revision += 1;
        Ok(next)
    }

    /// # Errors
    /// `42703` when the attribute is missing, `42P16` for a self-reference.
    pub fn with_retyped_attribute(
        &self,
        name: &str,
        data_type: DataType,
    ) -> Result<Self, TypeAlterError> {
        let index = self.require_attribute(name)?;
        self.reject_self_reference(&data_type)?;
        let mut next = self.clone();
        next.fields[index].data_type = data_type;
        next.revision += 1;
        Ok(next)
    }

    pub fn with_name(&self, canonical: String) -> Self {
        let mut next = self.clone();
        next.name = canonical;
        next.revision += 1;
        next
    }

    fn reject_self_reference(&self, data_type: &DataType) -> Result<(), TypeAlterError> {
        if data_type_depends_on(data_type, self.type_oid()) {
            return Err(TypeAlterError::new(
                "42P16",
                format!("composite type {} cannot be made a member of itself", self.local_name()),
            ));
        }
        Ok(())
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
