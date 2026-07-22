use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::runtime::QueryCancellationHandle;

const COMPLETED_OPERATION_LIMIT: usize = 1_024;

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::default()));

#[derive(Default)]
struct Registry {
    active: HashMap<Uuid, Arc<Operation>>,
    completed: VecDeque<(Uuid, String)>,
}

struct Operation {
    owner: String,
    cancellation: QueryCancellationHandle,
    completed: Notify,
}

pub(crate) struct Registration {
    id: Uuid,
    operation: Arc<Operation>,
    finished: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RegisterError {
    Duplicate,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CancelError {
    NotFound,
    AlreadyCompleted,
    TimedOut,
}

pub(crate) fn owner_fingerprint(token: Option<&str>) -> String {
    token.map_or_else(
        || "anonymous".to_string(),
        |token| format!("{:x}", Sha256::digest(token.as_bytes())),
    )
}

pub(crate) fn register(
    id: Uuid,
    owner: String,
    cancellation: QueryCancellationHandle,
) -> Result<Registration, RegisterError> {
    let mut registry = REGISTRY.lock().expect("query operation registry");
    if registry.active.contains_key(&id)
        || registry
            .completed
            .iter()
            .any(|(completed_id, _)| *completed_id == id)
    {
        return Err(RegisterError::Duplicate);
    }
    let operation = Arc::new(Operation {
        owner,
        cancellation,
        completed: Notify::new(),
    });
    registry.active.insert(id, Arc::clone(&operation));
    Ok(Registration {
        id,
        operation,
        finished: false,
    })
}

pub(crate) async fn cancel(id: Uuid, owner: &str, timeout: Duration) -> Result<(), CancelError> {
    let operation = {
        let registry = REGISTRY.lock().expect("query operation registry");
        if registry
            .completed
            .iter()
            .any(|(completed_id, completed_owner)| *completed_id == id && completed_owner == owner)
        {
            return Err(CancelError::AlreadyCompleted);
        }
        registry
            .active
            .get(&id)
            .filter(|operation| operation.owner == owner)
            .cloned()
            .ok_or(CancelError::NotFound)?
    };

    let completed = operation.completed.notified();
    operation.cancellation.cancel();
    tokio::time::timeout(timeout, completed)
        .await
        .map_err(|_| CancelError::TimedOut)
}

impl Registration {
    pub(crate) fn finish(mut self) {
        self.remove();
        self.finished = true;
    }

    fn remove(&self) {
        let mut registry = REGISTRY.lock().expect("query operation registry");
        registry.active.remove(&self.id);
        registry
            .completed
            .push_back((self.id, self.operation.owner.clone()));
        while registry.completed.len() > COMPLETED_OPERATION_LIMIT {
            registry.completed.pop_front();
        }
        self.operation.completed.notify_waiters();
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if !self.finished {
            self.operation.cancellation.cancel();
            self.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{cancel, owner_fingerprint, register, CancelError, RegisterError};
    use crate::runtime::QueryCancellationHandle;

    #[test]
    fn should_bind_cancellation_to_the_operation_owner_and_acknowledge_cleanup() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let id = uuid::Uuid::new_v4();
        let owner = owner_fingerprint(Some("session-cookie"));
        let cancellation = QueryCancellationHandle::new();
        let registration = register(id, owner.clone(), cancellation.clone()).expect("register");

        // Act
        runtime.block_on(async {
            assert_eq!(
                cancel(
                    id,
                    &owner_fingerprint(Some("other-cookie")),
                    Duration::from_millis(10)
                )
                .await,
                Err(CancelError::NotFound)
            );
            let acknowledged = async {
                tokio::task::yield_now().await;
                assert!(cancellation.is_cancelled());
                registration.finish();
            };
            let (result, ()) =
                tokio::join!(cancel(id, &owner, Duration::from_secs(1)), acknowledged);

            // Assert
            assert_eq!(result, Ok(()));
        });
    }

    #[test]
    fn should_reject_duplicate_and_completed_operation_ids() {
        // Arrange
        let id = uuid::Uuid::new_v4();
        let owner = owner_fingerprint(None);
        let registration =
            register(id, owner.clone(), QueryCancellationHandle::new()).expect("register");

        // Act / Assert
        assert!(matches!(
            register(id, owner.clone(), QueryCancellationHandle::new()),
            Err(RegisterError::Duplicate)
        ));
        registration.finish();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        assert_eq!(
            runtime.block_on(cancel(id, &owner, Duration::from_millis(10))),
            Err(CancelError::AlreadyCompleted)
        );
    }
}
