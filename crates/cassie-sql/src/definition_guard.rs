//! Small serialization helpers for versioned syntax declarations.

#[must_use]
pub const fn is_legacy<T: Default + PartialEq>(version: &T) -> bool {
    *version == T::default()
}
