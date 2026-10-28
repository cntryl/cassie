//! PostgreSQL 18 full-text search SQL behavior tracked by #799.

#[path = "table_predicate.rs"]
mod table_predicate;

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;
