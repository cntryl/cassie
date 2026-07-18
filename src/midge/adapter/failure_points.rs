use std::cell::Cell;
use std::sync::OnceLock;

use crate::app::CassieError;

static DOCUMENT_WRITE_FAILPOINT_TEST_GUARD: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();

thread_local! {
    static DOCUMENT_WRITE_FAILPOINT: Cell<u8> = const { Cell::new(0) };
    static DOCUMENT_WRITE_CONFLICTS_REMAINING: Cell<u8> = const { Cell::new(0) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum DocumentWriteFailurePoint {
    Row,
    ScalarIndex,
    TimeSeriesIndex,
    GraphAdjacency,
    NormalizedVector,
    VectorState,
}

impl DocumentWriteFailurePoint {
    const fn code(self) -> u8 {
        match self {
            Self::Row => 1,
            Self::ScalarIndex => 2,
            Self::TimeSeriesIndex => 3,
            Self::GraphAdjacency => 4,
            Self::NormalizedVector => 5,
            Self::VectorState => 6,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::ScalarIndex => "scalar-index",
            Self::TimeSeriesIndex => "time-series-index",
            Self::GraphAdjacency => "graph-adjacency",
            Self::NormalizedVector => "normalized-vector",
            Self::VectorState => "vector-state",
        }
    }
}

#[doc(hidden)]
pub fn set_document_write_failure_point(point: Option<DocumentWriteFailurePoint>) {
    DOCUMENT_WRITE_FAILPOINT.with(|failpoint| {
        failpoint.set(point.map_or(0, DocumentWriteFailurePoint::code));
    });
}

#[doc(hidden)]
pub fn document_write_failure_point_test_guard() -> parking_lot::MutexGuard<'static, ()> {
    DOCUMENT_WRITE_FAILPOINT_TEST_GUARD
        .get_or_init(|| parking_lot::Mutex::new(()))
        .lock()
}

pub(crate) fn check_document_write_failure_point(
    point: DocumentWriteFailurePoint,
) -> Result<(), CassieError> {
    let requested = DOCUMENT_WRITE_FAILPOINT.with(|failpoint| {
        if failpoint.get() != point.code() {
            return false;
        }
        failpoint.set(0);
        true
    });
    if !requested {
        return Ok(());
    }

    Err(CassieError::Execution(format!(
        "injected test failure after {} mutation",
        point.label()
    )))
}

#[doc(hidden)]
pub fn set_document_write_conflicts_remaining(remaining: u8) {
    DOCUMENT_WRITE_CONFLICTS_REMAINING.with(|counter| counter.set(remaining));
}

pub(crate) fn check_document_write_conflict_injection() -> Result<(), CassieError> {
    let injected = DOCUMENT_WRITE_CONFLICTS_REMAINING.with(|counter| {
        let remaining = counter.get();
        if remaining == 0 {
            return false;
        }
        counter.set(remaining.saturating_sub(1));
        true
    });
    if injected {
        return Err(CassieError::StorageRetryable(
            "midge write conflict: injected test conflict".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::{
        check_document_write_failure_point, set_document_write_failure_point,
        DocumentWriteFailurePoint,
    };

    #[test]
    fn should_isolate_document_write_failure_points_by_test_thread() {
        // Arrange
        let armed = Arc::new(Barrier::new(2));
        let checked = Arc::new(Barrier::new(2));
        let worker_armed = Arc::clone(&armed);
        let worker_checked = Arc::clone(&checked);
        let worker = std::thread::spawn(move || {
            set_document_write_failure_point(Some(DocumentWriteFailurePoint::TimeSeriesIndex));
            worker_armed.wait();
            worker_checked.wait();
            check_document_write_failure_point(DocumentWriteFailurePoint::TimeSeriesIndex)
        });
        armed.wait();

        // Act
        let unrelated =
            check_document_write_failure_point(DocumentWriteFailurePoint::TimeSeriesIndex);
        checked.wait();
        let injected = worker.join().expect("failpoint worker");

        // Assert
        assert!(unrelated.is_ok());
        assert!(injected
            .expect_err("armed thread should consume its failpoint")
            .to_string()
            .contains("time-series-index"));
    }
}
