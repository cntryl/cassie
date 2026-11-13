//! `ALTER TYPE` rules for enum types: pure descriptor transitions (#827).
//!
//! Every transition builds a new [`EnumTypeMeta`] with `revision` increased by
//! one; the previous value is never mutated. Member identities and the
//! relative order of existing members never change, so stored rows and index
//! keys stay valid across `ADD VALUE`, `RENAME VALUE` and a renumbering.
use super::composite_alter::TypeAlterError;
use super::enum_types::EnumTypeMeta;
use crate::types::enum_type::{sort_order_at, EnumMember, EnumValuePlacement};

/// Where and with what sort order `ADD VALUE` places the new member.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddValuePlan {
    /// Zero-based position in the ordered member list.
    pub index: usize,
    /// The new member's sort order, or `None` when no representable midpoint
    /// exists and every member is renumbered `1..=n`.
    pub sort_order: Option<f32>,
}

fn already_exists(label: &str) -> TypeAlterError {
    TypeAlterError {
        sqlstate: "42710",
        message: format!("enum label \"{label}\" already exists"),
    }
}

fn not_an_existing_label(label: &str) -> TypeAlterError {
    TypeAlterError {
        sqlstate: "22023",
        message: format!("\"{label}\" is not an existing enum label"),
    }
}

impl EnumTypeMeta {
    /// A copy renamed or moved to `canonical`.
    #[must_use]
    pub fn with_name(&self, canonical: String) -> Self {
        let mut next = self.clone();
        next.name = canonical;
        next.revision += 1;
        next
    }

    /// Validates `ADD VALUE 'label' [BEFORE | AFTER 'neighbour']` and decides
    /// the placement. PostgreSQL checks the duplicate before the neighbour.
    ///
    /// # Errors
    ///
    /// `42710` when the label exists, `22023` when the neighbour does not.
    pub fn plan_added_value(
        &self,
        label: &str,
        placement: Option<&EnumValuePlacement>,
    ) -> Result<AddValuePlan, TypeAlterError> {
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

    /// A copy with the new member inserted as `plan` decides. `uncommitted`
    /// marks a label that must not be used before the transaction commits.
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

    /// A copy whose member `from` is labelled `to`. The member identity and
    /// position are unchanged.
    ///
    /// # Errors
    ///
    /// `22023` when `from` does not exist, `42710` when `to` already does.
    pub fn with_renamed_value(&self, from: &str, to: &str) -> Result<Self, TypeAlterError> {
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

#[cfg(test)]
#[path = "enum_alter_tests.rs"]
mod tests;
