use super::{str, Arc, Cassie, CassieError};
#[cfg(debug_assertions)]
use std::collections::HashMap;
#[cfg(debug_assertions)]
use std::sync::{Mutex, OnceLock};
use tokio::task;

#[cfg(debug_assertions)]
static NEXT_RETRYABLE_FAILURES_FOR_TEST: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();

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

    #[cfg(debug_assertions)]
    if let Some(message) = take_retryable_failure_for_test(&cassie) {
        runtime.record_pgwire_boundary_error(operation_name, started_at.elapsed());
        return Err(CassieError::StorageRetryable(message));
    }

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

#[cfg(debug_assertions)]
pub(super) fn arm_next_retryable_failure_for_test(cassie: &Cassie, message: impl Into<String>) {
    retryable_failures_for_test()
        .lock()
        .expect("pgwire retryable failure test registry")
        .insert(runtime_key_for_test(cassie), message.into());
}

#[cfg(debug_assertions)]
fn take_retryable_failure_for_test(cassie: &Cassie) -> Option<String> {
    retryable_failures_for_test()
        .lock()
        .expect("pgwire retryable failure test registry")
        .remove(&runtime_key_for_test(cassie))
}

#[cfg(debug_assertions)]
fn retryable_failures_for_test() -> &'static Mutex<HashMap<usize, String>> {
    NEXT_RETRYABLE_FAILURES_FOR_TEST.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(debug_assertions)]
fn runtime_key_for_test(cassie: &Cassie) -> usize {
    Arc::as_ptr(&cassie.runtime).cast::<()>() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn data_dir(label: &str) -> String {
        format!("/tmp/cassie-pgwire-blocking-{label}-{}", Uuid::new_v4())
    }

    #[test]
    fn should_map_join_failures_to_retryable_storage_errors() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie =
                Arc::new(Cassie::new_with_data_dir(data_dir("retryable")).expect("cassie"));

            // Act
            let error =
                run_pgwire_blocking(cassie, "pgwire_retryable", |_| -> Result<(), CassieError> {
                    panic!("synthetic pgwire join failure");
                })
                .await
                .expect_err("panic should map to retryable storage");

            // Assert
            assert!(matches!(error, CassieError::StorageRetryable(_)));
            assert!(error
                .to_string()
                .contains("pgwire blocking boundary 'pgwire_retryable' failed"));
        });
    }
}
