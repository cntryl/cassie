//! PostgreSQL 18 mathematical operator grammar tracked by #813.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

include!("functions/math/math_operators.rs");

include!("functions/math/cube_root.rs");
