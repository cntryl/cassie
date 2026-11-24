use super::{str, Arc, Cassie, CassieError};
#[cfg(debug_assertions)]
use std::collections::HashMap;
#[cfg(debug_assertions)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
#[path = "blocking/cleanup_test_hooks.rs"]
pub(in crate::pgwire::connection) mod cleanup_test_hooks;

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
    run_pgwire_blocking_with_error(cassie, operation_name, operation).await
}

pub(super) async fn run_pgwire_blocking_with_error<T, E>(
    cassie: Arc<Cassie>,
    operation_name: &'static str,
    operation: impl FnOnce(Arc<Cassie>) -> Result<T, E> + Send + 'static,
) -> Result<T, E>
where
    T: Send + 'static,
    E: From<CassieError> + Send + 'static,
{
    let runtime = cassie.runtime().clone();
    let started_at = std::time::Instant::now();
    runtime.record_pgwire_boundary_started(operation_name);

    #[cfg(debug_assertions)]
    if let Some(message) = take_retryable_failure_for_test(&cassie) {
        runtime.record_pgwire_boundary_error(operation_name, started_at.elapsed());
        return Err(CassieError::StorageRetryable(message.into()).into());
    }

    let cleanup_cassie = Arc::clone(&cassie);
    #[cfg(test)]
    let probe_key = cleanup_test_hooks::key(&cassie);
    #[cfg(test)]
    let connection_admission = super::worker_lease::current_admission_for_test();
    let worker = super::worker_lease::spawn_owned_blocking(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(all(test, feature = "replay-benchmark-observation"))]
            {
                crate::midge::adapter::data_policy_test_observer::observe_worker(
                    &cleanup_cassie,
                    operation_name,
                    || operation(cassie),
                )
            }
            #[cfg(not(all(test, feature = "replay-benchmark-observation")))]
            {
                operation(cassie)
            }
        }));
        let recovery_allowed = cleanup_cassie.check_storage_recovery_required().is_ok();
        #[cfg(test)]
        let mut probe = cleanup_test_hooks::take(probe_key);
        #[cfg(test)]
        if let Some(probe) = probe.as_mut() {
            probe.before_cleanup(recovery_allowed, connection_admission);
        }
        if recovery_allowed {
            #[cfg(test)]
            if let Some(probe) = probe.as_ref() {
                probe.record_call();
            }
            let _ = cleanup_cassie.run_deferred_schema_cleanup();
        }
        #[cfg(test)]
        if let Some(probe) = probe {
            probe.finish();
        }
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    });
    #[cfg(test)]
    if cleanup_test_hooks::abort_before_start(probe_key) {
        worker.abort();
        cleanup_test_hooks::queued_abort_requested(probe_key);
    }
    let result = worker.await;

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
            Err(CassieError::StorageRetryable(
                format!("pgwire blocking boundary '{operation_name}' failed: {error}").into(),
            )
            .into())
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
    Arc::as_ptr(cassie.runtime()).cast::<()>() as usize
}

#[cfg(test)]
#[path = "blocking/tests.rs"]
mod tests;
