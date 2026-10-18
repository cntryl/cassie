//! PostgreSQL 18 query contracts that were missing from the focused owner suites.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

include!("../types/temporal/values_aggregate/semantics.rs");

include!("values_aggregate/nulls.rs");

include!("../types/arrays/values_aggregate/nulls.rs");
