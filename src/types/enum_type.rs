//! Named enum types (#827): immutable versioned descriptors and values.
//!
//! The design authority is the "Selected v0.4.0 named enum contract" section of
//! `docs/type-contract.md`. A stored enum value is its immutable member id; the
//! label and the order are resolved from the descriptor, so `RENAME VALUE` and
//! `ADD VALUE` never rewrite rows or index entries.
use std::ops::Deref;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The key of the stored JSON form `{"$cassie_enum_v1": <member_id>}`.
pub const ENUM_STORAGE_KEY: &str = "$cassie_enum_v1";
/// PostgreSQL's `NAMEDATALEN - 1`: the longest label in bytes.
pub const MAX_ENUM_LABEL_BYTES: usize = 63;

/// One label of an enum type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumMember {
    /// Immutable identity allocated from the object allocator; `pg_enum.oid`.
    pub member_id: u64,
    pub label: String,
    /// `pg_enum.enumsortorder`; strictly increasing along `members`.
    pub sort_order: f32,
    /// In-memory only: the label was added by `ADD VALUE` in the open
    /// transaction to a type that existed before it, so using it is `55P04`.
    /// Never persisted; the transaction publication clears it at commit.
    #[serde(skip)]
    pub uncommitted: bool,
}

/// Why a label is not an input value of an enum type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumInputError {
    /// `22P02`: no member has the label.
    InvalidLabel { type_name: String, label: String },
    /// `55P04`: the member was added in the open transaction to a type that
    /// existed before it.
    UncommittedLabel { type_name: String, label: String },
}

impl EnumInputError {
    #[must_use]
    pub const fn sqlstate(&self) -> &'static str {
        match self {
            Self::InvalidLabel { .. } => "22P02",
            Self::UncommittedLabel { .. } => "55P04",
        }
    }

    /// PostgreSQL 18's message text.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::InvalidLabel { type_name, label } => {
                format!("invalid input value for enum {type_name}: \"{label}\"")
            }
            Self::UncommittedLabel { type_name, label } => {
                format!("unsafe use of new value \"{label}\" of enum type {type_name}")
            }
        }
    }

    /// PostgreSQL 18's hint, if any.
    #[must_use]
    pub const fn hint(&self) -> Option<&'static str> {
        match self {
            Self::InvalidLabel { .. } => None,
            Self::UncommittedLabel { .. } => {
                Some("New enum values must be committed before they can be used.")
            }
        }
    }
}

/// An enum type descriptor. `members` is kept in sort order.
///
/// `revision` increases on every alteration. A descriptor is never mutated in
/// place; alterations build a new one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumType {
    /// Allocated database-scoped type OID.
    pub oid: u32,
    /// Allocated OID of the matching array type (`_name`).
    pub array_oid: u32,
    /// Canonical `database.schema.name`.
    pub qualified_name: String,
    /// Schema-local type name.
    pub name: String,
    pub revision: u64,
    pub members: Vec<EnumMember>,
}

/// Where `ALTER TYPE ... ADD VALUE` places a new label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnumValuePlacement {
    Before(String),
    After(String),
}

impl EnumType {
    /// The member with identity `member_id`.
    #[must_use]
    pub fn member(&self, member_id: u64) -> Option<&EnumMember> {
        self.members
            .iter()
            .find(|member| member.member_id == member_id)
    }

    /// The member labelled exactly `label` (labels are case-sensitive).
    #[must_use]
    pub fn member_by_label(&self, label: &str) -> Option<&EnumMember> {
        self.members.iter().find(|member| member.label == label)
    }

    /// Labels in sort order (`enum_range(NULL::t)`).
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|member| member.label.as_str())
    }

    /// The sort order a new label receives, or `None` when the neighbouring
    /// sort orders leave no representable midpoint and the type must first be
    /// renumbered with [`Self::renumbered`].
    ///
    /// Returns `Err(label)` naming the neighbour when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns the missing neighbour label.
    pub fn sort_order_for(
        &self,
        placement: Option<&EnumValuePlacement>,
    ) -> Result<Option<f32>, String> {
        let Some(placement) = placement else {
            return Ok(Some(
                self.members
                    .last()
                    .map_or(1.0, |member| member.sort_order + 1.0),
            ));
        };
        let neighbour = match placement {
            EnumValuePlacement::Before(label) | EnumValuePlacement::After(label) => label,
        };
        let index = self
            .members
            .iter()
            .position(|member| &member.label == neighbour)
            .ok_or_else(|| neighbour.clone())?;
        let (lower, upper) = match placement {
            EnumValuePlacement::Before(_) => (
                index.checked_sub(1).map(|lower| self.members[lower].sort_order),
                Some(self.members[index].sort_order),
            ),
            EnumValuePlacement::After(_) => (
                Some(self.members[index].sort_order),
                self.members.get(index + 1).map(|upper| upper.sort_order),
            ),
        };
        let candidate = match (lower, upper) {
            (Some(lower), Some(upper)) => lower + (upper - lower) / 2.0,
            (None, Some(upper)) => upper - 1.0,
            (Some(lower), None) => lower + 1.0,
            (None, None) => 1.0,
        };
        let above_lower = lower.is_none_or(|lower| candidate > lower);
        let below_upper = upper.is_none_or(|upper| candidate < upper);
        Ok((candidate.is_finite() && above_lower && below_upper).then_some(candidate))
    }

    /// A copy whose sort orders are `1..=n` in the current order. Member ids
    /// and relative order are unchanged.
    #[must_use]
    pub fn renumbered(&self) -> Self {
        let mut renumbered = self.clone();
        for (position, member) in renumbered.members.iter_mut().enumerate() {
            member.sort_order = sort_order_at(position);
        }
        renumbered
    }
}

/// The sort order `CREATE TYPE` assigns to the label at zero-based `position`.
#[must_use]
pub fn sort_order_at(position: usize) -> f32 {
    // Enum label counts are far below f32's exact-integer range (2^24).
    u16::try_from(position + 1).map_or(f32::MAX, f32::from)
}

impl EnumType {
    /// Whether `document` is the stored form of one of this type's members.
    #[must_use]
    pub fn has_storage_member(&self, document: &serde_json::Value) -> bool {
        EnumValue::storage_member_id(document).is_some_and(|id| self.member(id).is_some())
    }
}

/// Whether `data_type` is a named enum or an array of one.
#[must_use]
pub const fn is_enum_family(data_type: &super::DataType) -> bool {
    match data_type {
        super::DataType::Enum(_) => true,
        super::DataType::Array(element) => matches!(**element, super::DataType::Enum(_)),
        _ => false,
    }
}

/// A shared, serializable handle to an immutable descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumRef(Arc<EnumType>);

impl EnumRef {
    #[must_use]
    pub fn new(descriptor: EnumType) -> Self {
        Self(Arc::new(descriptor))
    }
}

impl Deref for EnumRef {
    type Target = EnumType;

    fn deref(&self) -> &EnumType {
        &self.0
    }
}

impl From<EnumType> for EnumRef {
    fn from(descriptor: EnumType) -> Self {
        Self::new(descriptor)
    }
}

impl Serialize for EnumRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EnumRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        EnumType::deserialize(deserializer).map(Self::new)
    }
}

/// A runtime enum value: its descriptor and member identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumValue {
    descriptor: EnumRef,
    member_id: u64,
}

impl EnumValue {
    /// A value of `descriptor`, or `None` when `member_id` is not one of its
    /// members.
    #[must_use]
    pub fn new(descriptor: EnumRef, member_id: u64) -> Option<Self> {
        descriptor.member(member_id)?;
        Some(Self {
            descriptor,
            member_id,
        })
    }

    /// `enum_in`: the value labelled `label`, or `None` for an unknown label
    /// (`22P02`).
    #[must_use]
    pub fn from_label(descriptor: &EnumRef, label: &str) -> Option<Self> {
        let member_id = descriptor.member_by_label(label)?.member_id;
        Some(Self {
            descriptor: descriptor.clone(),
            member_id,
        })
    }

    /// `enum_in` for SQL input (literals, casts, parameters, `enum_recv`):
    /// like [`Self::from_label`] but rejecting an uncommitted label.
    ///
    /// # Errors
    ///
    /// Returns `22P02` for an unknown label and `55P04` for an uncommitted one.
    pub fn parse_label(descriptor: &EnumRef, label: &str) -> Result<Self, EnumInputError> {
        let member = descriptor
            .member_by_label(label)
            .ok_or_else(|| EnumInputError::InvalidLabel {
                type_name: descriptor.name.clone(),
                label: label.to_string(),
            })?;
        if member.uncommitted {
            return Err(EnumInputError::UncommittedLabel {
                type_name: descriptor.name.clone(),
                label: label.to_string(),
            });
        }
        Ok(Self {
            descriptor: descriptor.clone(),
            member_id: member.member_id,
        })
    }

    #[must_use]
    pub const fn descriptor(&self) -> &EnumRef {
        &self.descriptor
    }

    #[must_use]
    pub const fn member_id(&self) -> u64 {
        self.member_id
    }

    fn member(&self) -> &EnumMember {
        self.descriptor
            .member(self.member_id)
            .expect("EnumValue is only constructed for a member of its descriptor")
    }

    /// `enum_out` and `enum_send`: the label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.member().label
    }

    /// The order key: the member's sort order under PostgreSQL's `float4`
    /// total order, then the member id so distinct members never tie.
    #[must_use]
    pub fn order_key(&self) -> (u32, u64) {
        (
            super::extended::real::order_key(self.member().sort_order),
            self.member_id,
        )
    }

    /// The stored JSON form, which is also the scalar-index key.
    #[must_use]
    pub fn to_storage_json(&self) -> serde_json::Value {
        serde_json::json!({ ENUM_STORAGE_KEY: self.member_id })
    }

    /// The member id of a stored JSON form, if `value` is one.
    #[must_use]
    pub fn storage_member_id(value: &serde_json::Value) -> Option<u64> {
        let object = value.as_object()?;
        if object.len() != 1 {
            return None;
        }
        object.get(ENUM_STORAGE_KEY)?.as_u64()
    }

    /// Heap bytes a clone retains (the descriptor is shared).
    #[must_use]
    pub const fn heap_bytes(&self) -> usize {
        0
    }
}

#[cfg(test)]
#[path = "enum_type_tests.rs"]
mod tests;
