//! PostgreSQL 18.6 residual aggregate and window interactions for #804 and #806.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

include!("../joins/aggregate_window_oracle/semantics.rs");

include!("windows/aggregate_window_oracle/semantics.rs");

include!("windows/aggregate_window_oracle/nulls.rs");
