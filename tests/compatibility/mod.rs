use super::compatibility_matrix::{db_error, CompatibilityServer};
use super::support_sql as support;
use cassie::app::Cassie;
use std::time::Duration;
use tokio_postgres::NoTls;

mod lifecycle;
#[path = "clients/dump_restore/pg_dump_restore.rs"]
mod pg_dump_restore;
#[path = "lifecycle/qualification_contracts.rs"]
mod postgresql18_lifecycle_inventory_wave2;
