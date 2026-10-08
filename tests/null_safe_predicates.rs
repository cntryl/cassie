//! The finite transient null-safe predicate contract; persisted definitions stay excluded.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "null_safe_predicates/scalar.rs"]
mod scalar;
#[path = "null_safe_predicates/syntax.rs"]
mod syntax;

#[path = "null_safe_predicates/definitions.rs"]
mod definitions;

#[path = "null_safe_predicates/arrays.rs"]
mod arrays;

#[path = "null_safe_predicates/restored_definitions.rs"]
mod restored_definitions;

#[path = "null_safe_predicates/runtime.rs"]
mod runtime;

#[path = "null_safe_predicates/loaded_materializations.rs"]
mod loaded_materializations;
