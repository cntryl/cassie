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

mod advisory_lock_disconnect;
mod copy_forms;
mod ddl_extensions;
mod exclusion_constraints;
mod mutation_ddl;
mod routine_objects;
mod session_transaction;
#[path = "../support/pgwire.rs"]
mod support_pgwire;
mod truncate_identity_rollback;
mod type_object_lifecycle;
