// Integration suite: domain types (#828 increment 2).
// Subsystem files live under `domain_types/`; shared fixtures come from
// `support/composite_types.rs` (restartable local storage) and `support/sql_fixture.rs`.

use cassie_test_support::support_composite_types;
use cassie_test_support::support_pgwire;
use cassie_test_support::support_sql_fixture;
use cassie_test_support::support_type_metadata_contract;

#[path = "domain_types/ddl.rs"]
mod ddl;

#[path = "domain_types/enforcement.rs"]
mod enforcement;

#[path = "domain_types/alter.rs"]
mod alter;

#[path = "domain_types/catalogs.rs"]
mod catalogs;

#[path = "domain_types/persistence.rs"]
mod persistence;

#[path = "domain_types/wire.rs"]
mod wire;
