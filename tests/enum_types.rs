// Integration suite: named enum types (#827, v0.4.0 contract in docs/type-contract.md).
// Subsystem files live under `enum_types/`; shared fixtures stay in `support/`.

use cassie_test_support::support_composite_types;
use cassie_test_support::support_enum_types;
use cassie_test_support::support_pgwire;
use cassie_test_support::support_sql_fixture;
use cassie_test_support::support_type_metadata_contract;

#[path = "enum_types/ddl.rs"]
mod ddl;

#[path = "enum_types/alter.rs"]
mod alter;

#[path = "enum_types/values.rs"]
mod values;

#[path = "enum_types/transactions.rs"]
mod transactions;

#[path = "enum_types/catalogs.rs"]
mod catalogs;

#[path = "enum_types/persistence.rs"]
mod persistence;

#[path = "enum_types/wire.rs"]
mod wire;

#[path = "enum_types/indexes.rs"]
mod indexes;

#[path = "enum_types/runtime/comparison.rs"]
mod runtime_comparison;

#[path = "enum_types/runtime/casts.rs"]
mod runtime_casts;

#[path = "enum_types/runtime/functions.rs"]
mod runtime_functions;

#[path = "enum_types/runtime/arrays.rs"]
mod runtime_arrays;

#[path = "enum_types/constraints.rs"]
mod constraints;

#[path = "enum_types/resolution.rs"]
mod resolution;

#[path = "enum_types/views.rs"]
mod views;
