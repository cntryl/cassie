use super::*;

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

async fn execute_statements(
    client: &tokio_postgres::Client,
    statements: &[&str],
) -> Result<(), tokio_postgres::Error> {
    for statement in statements {
        client.batch_execute(statement).await?;
    }
    Ok(())
}

struct DataDirectory(String);

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[path = "lifecycle/coordination/advisory_lock_disconnect.rs"]
mod advisory_lock_disconnect;
#[path = "lifecycle/aggregates/restart.rs"]
mod aggregate_restart;
#[path = "lifecycle/casts/prepared_dependencies.rs"]
mod cast_prepared_dependencies;
#[path = "lifecycle/casts/restart.rs"]
mod cast_restart;
#[path = "lifecycle/conversions/lifecycle.rs"]
mod conversion_lifecycle;
#[path = "lifecycle/copy/copy_forms.rs"]
mod copy_forms;
#[path = "lifecycle/ddl/ddl_extensions.rs"]
mod ddl_extensions;
#[path = "lifecycle/constraints/exclusion_constraints.rs"]
mod exclusion_constraints;
#[path = "lifecycle/large_objects/access_modes.rs"]
mod large_object_access_modes;
#[path = "lifecycle/large_objects/bounds.rs"]
mod large_object_bounds;
#[path = "lifecycle/large_objects/descriptor_lifetime.rs"]
mod large_object_descriptor_lifetime;
#[path = "lifecycle/large_objects/privileges.rs"]
mod large_object_privileges;
mod large_objects;
#[path = "lifecycle/mutations/merge_atomicity.rs"]
mod merge_atomicity;
#[path = "lifecycle/mutations/mutation_ddl.rs"]
mod mutation_ddl;
#[path = "lifecycle/operators/families.rs"]
mod operator_families;
#[path = "lifecycle/operators/restart.rs"]
mod operator_restart;
#[path = "lifecycle/routines/routine_objects.rs"]
mod routine_objects;
#[path = "lifecycle/transactions/row_lock_cancellation.rs"]
mod row_lock_cancellation;
#[path = "lifecycle/routines/rule_security.rs"]
mod rule_security;
#[path = "lifecycle/sessions/session_transaction.rs"]
mod session_transaction;
#[path = "lifecycle/statistics/privileges.rs"]
mod statistics_privileges;
use crate::support_pgwire;
#[path = "lifecycle/types/temporal_restart.rs"]
mod temporal_restart;
#[path = "lifecycle/sequences/truncate_identity_rollback.rs"]
mod truncate_identity_rollback;
#[path = "lifecycle/types/object_lifecycle.rs"]
mod type_object_lifecycle;
#[path = "lifecycle/mutations/upsert_concurrency.rs"]
mod upsert_concurrency;
