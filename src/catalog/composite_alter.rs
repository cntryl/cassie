//! Descriptor transformations behind `ALTER TYPE` (#828).
//!
//! A composite descriptor is immutable. Every change publishes a new revision
//! built from the previous one and then re-embeds the result in every object
//! that carries a copy: other composite types, table columns and views.
//!
//! The physical slot list only grows. Adding an attribute appends a slot,
//! dropping one records its slot in `dropped_positions`, so a value written
//! under any earlier revision is a prefix of the current layout.
use std::collections::HashMap;

use super::composite_types::{data_type_depends_on, CompositeTypeMeta};
use super::{local_name, Catalog};
use crate::types::composite::{CompositeField, CompositeRef};
use crate::types::DataType;

/// A rejected `ALTER TYPE` change with the PostgreSQL error class to report.
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
    fn attribute_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }

    fn require_attribute(&self, name: &str) -> Result<usize, TypeAlterError> {
        self.attribute_index(name).ok_or_else(|| {
            TypeAlterError::new(
                "42703",
                format!(
                    "column \"{name}\" of relation \"{}\" does not exist",
                    self.local_name()
                ),
            )
        })
    }

    fn reject_duplicate(&self, name: &str) -> Result<(), TypeAlterError> {
        if self
            .fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(name))
        {
            return Err(TypeAlterError::new(
                "42701",
                format!(
                    "column \"{name}\" of relation \"{}\" already exists",
                    self.local_name()
                ),
            ));
        }
        Ok(())
    }

    /// The next revision with `name` appended as a new physical slot.
    ///
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
        next.fields.push(CompositeField {
            name: name.to_owned(),
            data_type,
        });
        next.revision += 1;
        Ok(next)
    }

    /// The next revision with `name` hidden; its slot stays reserved.
    ///
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

    /// The next revision with attribute `from` renamed to `to`.
    ///
    /// # Errors
    /// `42703` when `from` is missing, `42701` when `to` exists.
    pub fn with_renamed_attribute(&self, from: &str, to: &str) -> Result<Self, TypeAlterError> {
        let index = self.require_attribute(from)?;
        if !from.eq_ignore_ascii_case(to) {
            self.reject_duplicate(to)?;
        }
        let mut next = self.clone();
        next.fields[index].name = to.to_owned();
        next.revision += 1;
        Ok(next)
    }

    /// The next revision with attribute `name` retyped. The caller has already
    /// proved that no stored value depends on the old type.
    ///
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

    /// The next revision under a new canonical name.
    #[must_use]
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
                format!(
                    "composite type {} cannot be made a member of itself",
                    self.local_name()
                ),
            ));
        }
        Ok(())
    }
}

/// `data_type` with every embedded composite replaced from `replacements`, or
/// `None` when nothing in it changes.
fn rebuild_data_type(
    data_type: &DataType,
    replacements: &HashMap<u32, CompositeRef>,
) -> Option<DataType> {
    match data_type {
        DataType::Composite(descriptor) => replacements
            .get(&descriptor.oid)
            .map(|replacement| DataType::Composite(replacement.clone())),
        DataType::Array(inner) => {
            rebuild_data_type(inner, replacements).map(|element| DataType::Array(Box::new(element)))
        }
        _ => None,
    }
}

/// One table column whose embedded descriptor must be re-published.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnRetype {
    pub table: String,
    pub column: String,
    pub data_type: DataType,
}

/// Every object that embeds a changed descriptor.
#[derive(Debug, Clone, Default)]
pub struct DescriptorPropagation {
    /// Other composite types whose fields embed the changed type.
    pub types: Vec<CompositeTypeMeta>,
    pub columns: Vec<ColumnRetype>,
    pub views: Vec<crate::catalog::ViewMeta>,
}

impl Catalog {
    /// Table columns, as `table.column`, that store `oid` directly or inside
    /// another composite type. PostgreSQL refuses a retype while any exists.
    #[must_use]
    pub fn composite_type_stored_in(&self, oid: u32) -> Vec<String> {
        let mut columns = Vec::new();
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if data_type_depends_on(&field.data_type, oid) {
                    columns.push(format!("{}.{}", local_name(&schema.collection), field.name));
                }
            }
        }
        columns.sort();
        columns
    }

    /// The objects that must be updated once `target` replaces its predecessor.
    #[must_use]
    pub fn propagate_composite_change(&self, target: &CompositeRef) -> DescriptorPropagation {
        let originals = self.list_composite_types();
        let mut replacements: HashMap<u32, CompositeRef> =
            HashMap::from([(target.oid, target.clone())]);
        let mut rebuilt: HashMap<u32, CompositeTypeMeta> = HashMap::new();
        loop {
            let mut changed = false;
            for original in &originals {
                if original.type_oid() == target.oid {
                    continue;
                }
                let mut next = original.clone();
                let mut touched = false;
                for field in &mut next.fields {
                    if let Some(data_type) = rebuild_data_type(&field.data_type, &replacements) {
                        field.data_type = data_type;
                        touched = true;
                    }
                }
                if !touched {
                    continue;
                }
                next.revision = original.revision + 1;
                let descriptor = next.descriptor();
                if replacements.get(&next.type_oid()) != Some(&descriptor) {
                    replacements.insert(next.type_oid(), descriptor);
                    rebuilt.insert(next.type_oid(), next);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut types = rebuilt.into_values().collect::<Vec<_>>();
        types.sort_by_key(|meta| meta.name.to_ascii_lowercase());
        DescriptorPropagation {
            types,
            columns: self.retyped_columns(&replacements),
            views: self.retyped_views(&replacements),
        }
    }

    fn retyped_columns(&self, replacements: &HashMap<u32, CompositeRef>) -> Vec<ColumnRetype> {
        let mut columns = Vec::new();
        for schema in self.schemas.read().values() {
            for field in &schema.fields {
                if let Some(data_type) = rebuild_data_type(&field.data_type, replacements) {
                    columns.push(ColumnRetype {
                        table: schema.collection.clone(),
                        column: field.name.clone(),
                        data_type,
                    });
                }
            }
        }
        columns
            .sort_by(|left, right| (&left.table, &left.column).cmp(&(&right.table, &right.column)));
        columns
    }

    fn retyped_views(
        &self,
        replacements: &HashMap<u32, CompositeRef>,
    ) -> Vec<crate::catalog::ViewMeta> {
        let mut views = Vec::new();
        for mut view in self.list_views() {
            let mut touched = false;
            for field in &mut view.schema.fields {
                if let Some(data_type) = rebuild_data_type(&field.data_type, replacements) {
                    field.data_type = data_type;
                    touched = true;
                }
            }
            if touched {
                views.push(view);
            }
        }
        views
    }
}

#[cfg(test)]
#[path = "composite_alter_tests.rs"]
mod tests;
