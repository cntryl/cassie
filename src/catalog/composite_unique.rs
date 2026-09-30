//! Multi-column `UNIQUE (a, b)` constraints.
//!
//! Each column of a composite UNIQUE constraint carries the constraint's name
//! and its position (`unique_name`, `unique_ordinal`) for catalog reporting,
//! but the constraint is enforced on the whole tuple through a backing unique
//! scalar index with the constraint's name, exactly as PostgreSQL backs a
//! UNIQUE constraint with an index. The per-column UNIQUE paths therefore skip
//! composite members, so rows that share only one of the columns are legal.

use std::collections::BTreeMap;

use super::{FieldConstraint, IndexKind, IndexMeta};

/// One composite UNIQUE constraint: its name and its columns in declared order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeUnique {
    pub name: String,
    pub fields: Vec<String>,
}

/// Returns the composite UNIQUE constraints declared across `constraints`:
/// every `unique_name` that two or more UNIQUE columns share.
#[must_use]
pub fn composite_unique_constraints(constraints: &[FieldConstraint]) -> Vec<CompositeUnique> {
    let mut groups = BTreeMap::<String, (String, Vec<(u32, String)>)>::new();
    for constraint in constraints.iter().filter(|constraint| constraint.unique) {
        let Some(name) = constraint.unique_name.as_ref() else {
            continue;
        };
        groups
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| (name.clone(), Vec::new()))
            .1
            .push((
                constraint.unique_ordinal.unwrap_or(u32::MAX),
                constraint.field.clone(),
            ));
    }
    groups
        .into_values()
        .filter(|(_, fields)| fields.len() > 1)
        .map(|(name, mut fields)| {
            fields.sort();
            CompositeUnique {
                name,
                fields: fields.into_iter().map(|(_, field)| field).collect(),
            }
        })
        .collect()
}

/// Whether `constraint`'s UNIQUE flag belongs to a composite constraint, in
/// which case uniqueness is enforced by the backing index rather than on the
/// column alone.
#[must_use]
pub fn is_composite_unique_member(
    constraint: &FieldConstraint,
    constraints: &[FieldConstraint],
) -> bool {
    constraint.unique
        && constraint.unique_name.as_ref().is_some_and(|name| {
            constraints
                .iter()
                .filter(|other| {
                    other.unique
                        && other
                            .unique_name
                            .as_ref()
                            .is_some_and(|other_name| other_name.eq_ignore_ascii_case(name))
                })
                .nth(1)
                .is_some()
        })
}

/// Whether `constraint` makes its column unique on its own: a PRIMARY KEY
/// column, or a UNIQUE column that is not part of a composite constraint.
#[must_use]
pub fn enforces_single_column_uniqueness(
    constraint: &FieldConstraint,
    constraints: &[FieldConstraint],
) -> bool {
    constraint.primary_key
        || (constraint.unique && !is_composite_unique_member(constraint, constraints))
}

/// The unique scalar indexes that enforce the composite UNIQUE constraints in
/// `constraints` on `collection`, named after their constraints.
#[must_use]
pub fn composite_unique_indexes(
    collection: &str,
    constraints: &[FieldConstraint],
) -> Vec<IndexMeta> {
    composite_unique_constraints(constraints)
        .into_iter()
        .map(|composite| IndexMeta {
            collection: collection.to_string(),
            name: composite.name,
            field: composite.fields[0].clone(),
            fields: composite.fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: true,
            options: BTreeMap::new(),
        })
        .collect()
}

/// Returns a message when `additions` would put a column into a second
/// UNIQUE constraint while either constraint spans several columns. Each
/// column records one UNIQUE membership, so such a declaration cannot be
/// represented and is rejected instead of silently replacing the first one.
#[must_use]
pub fn conflicting_unique_membership(
    collection: &str,
    existing: &[FieldConstraint],
    additions: &[FieldConstraint],
) -> Option<String> {
    let mut combined = existing.to_vec();
    combined.extend(additions.iter().cloned());
    for addition in additions.iter().filter(|addition| addition.unique) {
        let Some(current) = existing
            .iter()
            .find(|entry| entry.unique && entry.field.eq_ignore_ascii_case(&addition.field))
        else {
            continue;
        };
        let current_name = unique_name(collection, current);
        let added_name = unique_name(collection, addition);
        if current_name.eq_ignore_ascii_case(&added_name) {
            continue;
        }
        let spans_columns = |name: &str| {
            combined
                .iter()
                .filter(|entry| {
                    entry.unique && unique_name(collection, entry).eq_ignore_ascii_case(name)
                })
                .nth(1)
                .is_some()
        };
        if spans_columns(&current_name) || spans_columns(&added_name) {
            return Some(format!(
                "column '{}' cannot be in both UNIQUE constraint '{current_name}' and '{added_name}': a column may belong to only one multi-column UNIQUE constraint",
                addition.field
            ));
        }
    }
    None
}

fn unique_name(collection: &str, constraint: &FieldConstraint) -> String {
    constraint.unique_name.clone().unwrap_or_else(|| {
        super::generated_constraint_name(collection, &constraint.field, "UNIQUE")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        composite_unique_constraints, conflicting_unique_membership,
        enforces_single_column_uniqueness, CompositeUnique,
    };
    use crate::catalog::FieldConstraint;

    fn unique(field: &str, name: Option<&str>, ordinal: u32) -> FieldConstraint {
        let mut constraint = FieldConstraint::new(field);
        constraint.unique = true;
        constraint.unique_name = name.map(str::to_string);
        constraint.unique_ordinal = Some(ordinal);
        constraint
    }

    #[test]
    fn should_group_columns_that_share_a_unique_constraint_name() {
        // Arrange
        let constraints = vec![
            unique("b", Some("t_ab"), 2),
            unique("a", Some("T_AB"), 1),
            unique("c", Some("t_c"), 1),
            unique("d", None, 1),
        ];

        // Act
        let composites = composite_unique_constraints(&constraints);

        // Assert
        assert_eq!(
            composites,
            vec![CompositeUnique {
                name: "t_ab".to_string(),
                fields: vec!["a".to_string(), "b".to_string()],
            }]
        );
        assert!(!enforces_single_column_uniqueness(
            &constraints[0],
            &constraints
        ));
        assert!(enforces_single_column_uniqueness(
            &constraints[2],
            &constraints
        ));
        assert!(enforces_single_column_uniqueness(
            &constraints[3],
            &constraints
        ));
    }

    #[test]
    fn should_reject_a_column_in_two_unique_constraints_when_one_is_composite() {
        // Arrange
        let existing = vec![unique("a", None, 1)];
        let composite = vec![unique("a", Some("t_ab"), 1), unique("b", Some("t_ab"), 2)];
        let same_column_again = vec![unique("a", Some("t_a_again"), 1)];

        // Act
        let composite_conflict = conflicting_unique_membership("t", &existing, &composite);
        let single_conflict = conflicting_unique_membership("t", &existing, &same_column_again);

        // Assert
        assert!(composite_conflict.is_some());
        assert!(single_conflict.is_none());
    }
}
