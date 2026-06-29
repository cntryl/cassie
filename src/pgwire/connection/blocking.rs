use super::{Arc, Cassie, str, CassieError};
use tokio::task;

pub(super) async fn run_pgwire_blocking<T>(
    cassie: Arc<Cassie>,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>) -> Result<T, CassieError> + Send + 'static,
) -> Result<T, CassieError>
where
    T: Send + 'static,
{
    let runtime = cassie.runtime.clone();
    let started_at = std::time::Instant::now();
    runtime.record_pgwire_boundary_started(operation_name);

    let result = task::spawn_blocking(move || operation(cassie)).await;

    match result {
        Ok(result) => match result {
            Ok(value) => {
                runtime.record_pgwire_boundary_completed(operation_name, started_at.elapsed());
                Ok(value)
            }
            Err(error) => {
                runtime.record_pgwire_boundary_error(operation_name, started_at.elapsed());
                Err(error)
            }
        },
        Err(error) => {
            runtime.record_pgwire_boundary_join_failed(operation_name, started_at.elapsed());
            Err(CassieError::StorageRetryable(format!(
                "pgwire blocking boundary '{operation_name}' failed: {error}"
            )))
        }
    }
}
