//! Composite and single-column uniqueness declarations.

pub use cassie_sql::composite_unique::{
    composite_primary_key_constraints, composite_primary_key_indexes, composite_unique_constraints,
    composite_unique_indexes, conflicting_unique_membership, enforces_single_column_uniqueness,
    is_composite_primary_key_member, is_composite_unique_member, unique_constraint_indexes,
    CompositeUnique,
};
