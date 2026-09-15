use std::cell::Cell;
use std::sync::{Arc, Barrier, Mutex, OnceLock};

type SchemaWriteCommitControl = (SchemaWritePausePoint, Arc<Barrier>, Arc<Barrier>);

static SCHEMA_WRITE_COMMIT_CONTROL: OnceLock<Mutex<Option<SchemaWriteCommitControl>>> =
    OnceLock::new();
static SCHEMA_WRITE_CONFLICT_TEST_GUARD: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();
thread_local! {
    static SCHEMA_WRITE_CONFLICT_WORKER: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum SchemaWritePausePoint {
    CollectionCreate,
    DatabaseCreateFinalize,
    SequenceNextValue,
}

#[doc(hidden)]
pub struct SchemaWriteConflictTestGuard {
    _guard: parking_lot::MutexGuard<'static, ()>,
}

#[doc(hidden)]
pub struct SchemaWriteConflictWorkerGuard {
    previous: bool,
}

impl Drop for SchemaWriteConflictWorkerGuard {
    fn drop(&mut self) {
        SCHEMA_WRITE_CONFLICT_WORKER.set(self.previous);
    }
}

impl Drop for SchemaWriteConflictTestGuard {
    fn drop(&mut self) {
        set_schema_write_commit_barriers(None, None, None);
    }
}

#[doc(hidden)]
#[must_use]
pub fn schema_write_conflict_test_guard() -> SchemaWriteConflictTestGuard {
    SchemaWriteConflictTestGuard {
        _guard: SCHEMA_WRITE_CONFLICT_TEST_GUARD
            .get_or_init(|| parking_lot::Mutex::new(()))
            .lock(),
    }
}

#[doc(hidden)]
#[must_use]
pub fn schema_write_conflict_worker_guard() -> SchemaWriteConflictWorkerGuard {
    let previous = SCHEMA_WRITE_CONFLICT_WORKER.replace(true);
    SchemaWriteConflictWorkerGuard { previous }
}

#[doc(hidden)]
pub fn set_schema_write_commit_barriers(
    pause_point: Option<SchemaWritePausePoint>,
    ready: Option<Arc<Barrier>>,
    resume: Option<Arc<Barrier>>,
) {
    *SCHEMA_WRITE_COMMIT_CONTROL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("schema write commit barrier mutex") = pause_point
        .zip(ready)
        .zip(resume)
        .map(|((point, ready), resume)| (point, ready, resume));
}

pub(super) fn pause_before_schema_write_commit(pause_point: SchemaWritePausePoint) {
    if !SCHEMA_WRITE_CONFLICT_WORKER.get() {
        return;
    }
    let control = SCHEMA_WRITE_COMMIT_CONTROL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("schema write commit barrier mutex")
        .clone();
    if let Some((configured_point, ready, resume)) = control {
        if configured_point == pause_point {
            ready.wait();
            resume.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn should_pause_only_explicit_schema_conflict_workers() {
        // Arrange
        let _test_guard = schema_write_conflict_test_guard();
        let ready = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        set_schema_write_commit_barriers(
            Some(SchemaWritePausePoint::CollectionCreate),
            Some(Arc::clone(&ready)),
            Some(Arc::clone(&resume)),
        );
        let (unscoped_tx, unscoped_rx) = mpsc::channel();

        // Act
        let unscoped = std::thread::spawn(move || {
            pause_before_schema_write_commit(SchemaWritePausePoint::CollectionCreate);
            unscoped_tx.send(()).expect("report unscoped completion");
        });
        unscoped_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("unscoped schema writer must not consume the test barrier");
        let scoped = std::thread::spawn(move || {
            let _worker = schema_write_conflict_worker_guard();
            pause_before_schema_write_commit(SchemaWritePausePoint::CollectionCreate);
        });
        ready.wait();
        resume.wait();

        // Assert
        unscoped.join().expect("join unscoped schema writer");
        scoped.join().expect("join scoped schema writer");
    }
}
