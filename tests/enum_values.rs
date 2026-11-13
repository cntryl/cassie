// Integration suite: named enum value semantics (#827, value-layer lane).
// Subsystem files live under `enum_values/`; shared fixtures stay in `support/`.

use cassie_test_support::support_sql_fixture;

#[path = "enum_values/comparison.rs"]
mod comparison;

#[path = "enum_values/casts.rs"]
mod casts;

#[path = "enum_values/functions.rs"]
mod functions;

#[path = "enum_values/arrays.rs"]
mod arrays;
