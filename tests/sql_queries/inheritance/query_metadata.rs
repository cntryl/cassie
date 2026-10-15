//! Inherited parent scan descriptors and tableoid predicates tracked by #909.

use cassie::types::Value;

use super::super::support_sql_fixture::sql_fixture;

include!("../projection/inheritance_query/metadata.rs");

include!("inheritance_query/metadata.rs");
