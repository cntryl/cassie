//! Populated PostgreSQL type and object-identity catalog contracts for #817/#818.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

include!("../../sequences/catalog/metadata.rs");

include!("../../types/catalog/metadata.rs");

include!("../../types/catalog/semantics.rs");
