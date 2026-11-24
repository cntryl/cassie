//! Multi-column `UNIQUE (a, b)` constraints.
//!
//! Each column of a composite UNIQUE constraint carries the constraint's name
//! and its position (`unique_name`, `unique_ordinal`) for catalog reporting,
//! but the constraint is enforced on the whole tuple through a backing unique
//! scalar index with the constraint's name, exactly as PostgreSQL backs a
//! UNIQUE constraint with an index. The per-column UNIQUE paths therefore skip
//! composite members, so rows that share only one of the columns are legal.

use std::collections::BTreeMap;

use cassie_core::catalog::declarations::UniqueNullPolicy;

use crate::catalog_declarations::{IndexKind, IndexMeta};
use crate::column_identifier::ColumnIdentifierPath;
use crate::declarations::FieldConstraint;

/// One composite UNIQUE constraint: its name and its columns in declared order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeUnique {
    pub name: String,
    pub fields: Vec<String>,
}

/// Returns the composite PRIMARY KEY constraints declared across `constraints`.
#[must_use]
pub fn composite_primary_key_constraints<A>(
    constraints: &[FieldConstraint<A>],
) -> Vec<CompositeUnique> {
    let mut groups = BTreeMap::<String, (String, Vec<(u32, String)>)>::new();
    for constraint in constraints
        .iter()
        .filter(|constraint| constraint.primary_key)
    {
        let Some(name) = constraint.primary_key_name.as_ref() else {
            continue;
        };
        let fields = &mut groups
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| (name.clone(), Vec::new()))
            .1;
        let field_key = ColumnIdentifierPath::stored_field_key(&constraint.field);
        if fields
            .iter()
            .any(|(_, field)| ColumnIdentifierPath::stored_field_key(field) == field_key)
        {
            continue;
        }
        fields.push((
            constraint.primary_key_ordinal.unwrap_or(u32::MAX),
            constraint.field.clone(),
        ));
    }
    groups
        .into_values()
        .filter(|(_, fields)| fields.len() > 1)
        .map(|(name, mut fields)| {
            fields.sort_by_key(|(ordinal, _)| *ordinal);
            CompositeUnique {
                name,
                fields: fields.into_iter().map(|(_, field)| field).collect(),
            }
        })
        .collect()
}

/// Whether a PRIMARY KEY member is enforced as part of a composite key.
#[must_use]
pub fn is_composite_primary_key_member<A>(
    constraint: &FieldConstraint<A>,
    constraints: &[FieldConstraint<A>],
) -> bool {
    constraint.primary_key
        && constraint.primary_key_name.as_ref().is_some_and(|name| {
            constraints
                .iter()
                .filter(|other| {
                    other.primary_key
                        && other
                            .primary_key_name
                            .as_ref()
                            .is_some_and(|other_name| other_name.eq_ignore_ascii_case(name))
                })
                .map(|other| ColumnIdentifierPath::stored_field_key(&other.field))
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1
        })
}

/// Returns the composite UNIQUE constraints declared across `constraints`:
/// every `unique_name` that two or more UNIQUE columns share.
#[must_use]
pub fn composite_unique_constraints<A>(constraints: &[FieldConstraint<A>]) -> Vec<CompositeUnique> {
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
            fields.sort_by_key(|(ordinal, _)| *ordinal);
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
pub fn is_composite_unique_member<A>(
    constraint: &FieldConstraint<A>,
    constraints: &[FieldConstraint<A>],
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
pub fn enforces_single_column_uniqueness<A>(
    constraint: &FieldConstraint<A>,
    constraints: &[FieldConstraint<A>],
) -> bool {
    (constraint.primary_key && !is_composite_primary_key_member(constraint, constraints))
        || (constraint.unique && !is_composite_unique_member(constraint, constraints))
}

/// Unique scalar indexes backing composite PRIMARY KEY constraints.
#[must_use]
pub fn composite_primary_key_indexes<A>(
    collection: &str,
    constraints: &[FieldConstraint<A>],
) -> Vec<IndexMeta> {
    composite_primary_key_constraints(constraints)
        .into_iter()
        .map(|composite| IndexMeta {
            definition_semantics_version: 0,
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

/// The unique scalar indexes that enforce the composite UNIQUE constraints in
/// `constraints` on `collection`, named after their constraints.
#[must_use]
pub fn composite_unique_indexes<A>(
    collection: &str,
    constraints: &[FieldConstraint<A>],
) -> Vec<IndexMeta> {
    composite_unique_constraints(constraints)
        .into_iter()
        .filter_map(|composite| {
            let first_member = constraints.iter().find(|constraint| {
                constraint.unique
                    && constraint.unique_ordinal == Some(1)
                    && constraint
                        .unique_name
                        .as_ref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&composite.name))
            });
            if first_member.is_some_and(|constraint| constraint.constraint_index_name.is_some()) {
                return None;
            }
            let nulls_not_distinct = constraints.iter().any(|constraint| {
                constraint.unique
                    && constraint.unique_null_policy == UniqueNullPolicy::NotDistinct
                    && constraint
                        .unique_name
                        .as_ref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&composite.name))
            });
            let mut options = BTreeMap::new();
            if nulls_not_distinct {
                options.insert(
                    IndexMeta::NULLS_NOT_DISTINCT_OPTION.to_string(),
                    "true".to_string(),
                );
            }
            if let Some(first_member) = first_member {
                options.extend(first_member.constraint_index_options.clone());
            }
            Some(IndexMeta {
                definition_semantics_version: 0,
                collection: collection.to_string(),
                name: composite.name,
                field: composite.fields[0].clone(),
                fields: composite.fields,
                expressions: Vec::new(),
                include_fields: first_member.map_or_else(Vec::new, |constraint| {
                    constraint.constraint_index_include_fields.clone()
                }),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: true,
                options,
            })
        })
        .collect()
}

/// Returns the scalar indexes that back every single-column and composite
/// UNIQUE constraint on `collection`.
#[must_use]
pub fn unique_constraint_indexes<A>(
    collection: &str,
    constraints: &[FieldConstraint<A>],
) -> Vec<IndexMeta> {
    let mut indexes = constraints
        .iter()
        .filter(|constraint| {
            constraint.unique
                && !constraint.primary_key
                && constraint.constraint_index_name.is_none()
                && !is_composite_unique_member(constraint, constraints)
        })
        .map(|constraint| {
            let name = constraint.unique_constraint_name(collection);
            let mut options = BTreeMap::new();
            if constraint.unique_null_policy == UniqueNullPolicy::NotDistinct {
                options.insert(
                    IndexMeta::NULLS_NOT_DISTINCT_OPTION.to_string(),
                    "true".to_string(),
                );
            }
            options.extend(constraint.constraint_index_options.clone());
            IndexMeta {
                definition_semantics_version: 0,
                collection: collection.to_string(),
                name,
                field: constraint.field.clone(),
                fields: vec![constraint.field.clone()],
                expressions: Vec::new(),
                include_fields: constraint.constraint_index_include_fields.clone(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: true,
                options,
            }
        })
        .collect::<Vec<_>>();
    indexes.extend(composite_unique_indexes(collection, constraints));
    indexes
}

pub use crate::declarations::conflicting_unique_membership;

#[cfg(test)]
mod tests {
    use super::{
        composite_unique_constraints, conflicting_unique_membership,
        enforces_single_column_uniqueness, CompositeUnique,
    };
    type FieldConstraint = crate::declarations::FieldConstraint<()>;

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
