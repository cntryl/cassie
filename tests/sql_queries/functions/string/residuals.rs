//! Residual PostgreSQL 18 string edge cases tracked by #803.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

include!("nulls.rs");

include!("semantics.rs");
