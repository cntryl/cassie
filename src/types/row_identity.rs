//! Names and rules for Cassie's internal row identity.
//!
//! Every stored document has an internal identity. Rows carry it under the
//! reserved [`ROW_IDENTITY_COLUMN`] (`_id`) key only, and `_id` always means
//! that identity. A bare [`LEGACY_ID_COLUMN`] (`id`) is an alias for the
//! identity only when the target relation is a base table that declares no
//! `id` column of its own; once a table declares `id`, that name is an
//! ordinary column everywhere. All comparisons are ASCII case-insensitive,
//! matching SQL identifier resolution.

/// The reserved name that always resolves to the internal row identity.
pub const ROW_IDENTITY_COLUMN: &str = "_id";

/// The legacy alias for the internal row identity, used only when the target
/// base table declares no column of that name.
pub const LEGACY_ID_COLUMN: &str = "id";

/// Whether `name` is the reserved internal row identity (`_id`).
#[must_use]
pub fn is_row_identity_column(name: &str) -> bool {
    name.eq_ignore_ascii_case(ROW_IDENTITY_COLUMN)
}

/// Whether `name` is spelled like the legacy `id` alias.
#[must_use]
pub fn is_legacy_id_column(name: &str) -> bool {
    name.eq_ignore_ascii_case(LEGACY_ID_COLUMN)
}

/// Whether a relation with these declared field names declares its own `id`
/// column, which then shadows the legacy identity alias.
#[must_use]
pub fn declares_id<'a>(field_names: impl IntoIterator<Item = &'a str>) -> bool {
    field_names.into_iter().any(is_legacy_id_column)
}

/// Whether a reference to `field` means the internal row identity against a
/// relation that does (`declares_id == true`) or does not declare its own
/// `id` column.
#[must_use]
pub fn is_identity_reference(field: &str, declares_id: bool) -> bool {
    is_row_identity_column(field) || (!declares_id && is_legacy_id_column(field))
}

#[cfg(test)]
mod tests {
    use super::{declares_id, is_identity_reference, is_row_identity_column};

    #[test]
    fn should_treat_underscore_id_as_identity_regardless_of_declared_columns() {
        // Arrange
        let names = ["_id", "_ID"];

        // Act
        let resolved = names
            .iter()
            .map(|name| {
                (
                    is_row_identity_column(name),
                    is_identity_reference(name, true),
                    is_identity_reference(name, false),
                )
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(resolved.iter().all(|flags| *flags == (true, true, true)));
    }

    #[test]
    fn should_treat_bare_id_as_identity_only_without_a_declared_id() {
        // Arrange
        let declared = declares_id(["name", "ID"]);
        let undeclared = declares_id(["name"]);

        // Act
        let with_declared = is_identity_reference("Id", declared);
        let without_declared = is_identity_reference("Id", undeclared);

        // Assert
        assert!(declared);
        assert!(!undeclared);
        assert!(!with_declared);
        assert!(without_declared);
    }
}
