//! `ALTER TYPE` rules for enum types: pure descriptor transitions (#827).
//!
//! Every transition builds a new [`cassie_core::catalog::enum_type::EnumTypeMeta`] with `revision` increased by
//! one; the previous value is never mutated. Member identities and the
//! relative order of existing members never change, so stored rows and index
//! keys stay valid across `ADD VALUE`, `RENAME VALUE` and a renumbering.
pub use cassie_core::catalog::composite_type::TypeAlterError;
pub use cassie_core::catalog::enum_type::AddValuePlan;

#[cfg(test)]
#[path = "enum_alter_tests.rs"]
mod tests;
