use std::sync::{Arc, Barrier, Mutex, OnceLock};

type SchemaWriteCommitControl = (SchemaWritePausePoint, Arc<Barrier>, Arc<Barrier>);

static SCHEMA_WRITE_COMMIT_CONTROL: OnceLock<Mutex<Option<SchemaWriteCommitControl>>> =
    OnceLock::new();
static SCHEMA_WRITE_CONFLICT_TEST_GUARD: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();

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
