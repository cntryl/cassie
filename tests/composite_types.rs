// Integration suite: composite types and ROW values (#828 increment 1).
// Subsystem files live under `composite_types/`; shared fixtures stay in `support/`.

use cassie_test_support::support_composite_types;
use cassie_test_support::support_pgwire;
use cassie_test_support::support_sql_fixture;
use cassie_test_support::support_type_metadata_contract;

#[path = "composite_types/ddl.rs"]
mod ddl;

#[path = "composite_types/alter.rs"]
mod alter;

#[path = "composite_types/row_types.rs"]
mod row_types;

#[path = "composite_types/values.rs"]
mod values;

#[path = "composite_types/records.rs"]
mod records;

#[path = "composite_types/catalogs.rs"]
mod catalogs;

#[path = "composite_types/persistence.rs"]
mod persistence;

#[path = "composite_types/wire.rs"]
mod wire;
