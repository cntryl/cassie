//! Portable named enum type declarations.
use crate::catalog::scope::{local_name, relation_schema_name};
use crate::types::enum_type::{sort_order_at, EnumMember, EnumRef, EnumType, EnumValuePlacement};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddValuePlan {
    /// Zero-based position in the ordered member list.
    pub index: usize,
    /// New sort key, or `None` when members must be renumbered.
    pub sort_order: Option<f32>,
}

fn alter_error(sqlstate: &'static str, message: String) -> super::composite_type::TypeAlterError {
    super::composite_type::TypeAlterError { sqlstate, message }
}

fn already_exists(label: &str) -> super::composite_type::TypeAlterError {
    alter_error("42710", format!("enum label \"{label}\" already exists"))
}

fn not_an_existing_label(label: &str) -> super::composite_type::TypeAlterError {
    alter_error(
        "22023",
        format!("\"{label}\" is not an existing enum label"),
    )
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

    /// Returns a copy under the new canonical name.
    #[must_use]
    pub fn with_name(&self, canonical: String) -> Self {
        let mut next = self.clone();
        next.name = canonical;
        next.revision += 1;
        next
    }

    /// # Errors
    /// `42710` for a duplicate label, `22023` for an absent neighbor.
    pub fn plan_added_value(
        &self,
        label: &str,
        placement: Option<&EnumValuePlacement>,
    ) -> Result<AddValuePlan, super::composite_type::TypeAlterError> {
        if self.members.iter().any(|member| member.label == label) {
            return Err(already_exists(label));
        }
        let sort_order = self
            .descriptor()
            .sort_order_for(placement)
            .map_err(|neighbour| not_an_existing_label(&neighbour))?;
        let neighbour_index = |neighbour: &str| {
            self.members
                .iter()
                .position(|member| member.label == neighbour)
        };
        let index = match placement {
            None => self.members.len(),
            Some(EnumValuePlacement::Before(neighbour)) => {
                neighbour_index(neighbour).ok_or_else(|| not_an_existing_label(neighbour))?
            }
            Some(EnumValuePlacement::After(neighbour)) => {
                neighbour_index(neighbour).ok_or_else(|| not_an_existing_label(neighbour))? + 1
            }
        };
        Ok(AddValuePlan { index, sort_order })
    }

    /// Returns a copy with the member inserted at the planned position.
    #[must_use]
    pub fn with_added_value(
        &self,
        plan: AddValuePlan,
        label: String,
        member_id: u64,
        uncommitted: bool,
    ) -> Self {
        let mut next = self.clone();
        next.revision += 1;
        next.members.insert(
            plan.index,
            EnumMember {
                member_id,
                label,
                sort_order: plan.sort_order.unwrap_or(0.0),
                uncommitted,
            },
        );
        if plan.sort_order.is_none() {
            for (position, member) in next.members.iter_mut().enumerate() {
                member.sort_order = sort_order_at(position);
            }
        }
        next
    }

    /// # Errors
    /// `22023` if `from` is missing, `42710` if `to` already exists.
    pub fn with_renamed_value(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Self, super::composite_type::TypeAlterError> {
        let index = self
            .members
            .iter()
            .position(|member| member.label == from)
            .ok_or_else(|| not_an_existing_label(from))?;
        if self.members.iter().any(|member| member.label == to) {
            return Err(already_exists(to));
        }
        let mut next = self.clone();
        next.revision += 1;
        to.clone_into(&mut next.members[index].label);
        Ok(next)
    }
}
