//! Direct catalog and system-information lookups tracked by #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

include!("../relations/catalog_lookup.rs");

include!("../routines/catalog_lookup.rs");

include!("../sequences/catalog_lookup.rs");

include!("../sizes/catalog_lookup.rs");

include!("../system_information/catalog_lookup.rs");
