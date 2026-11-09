//! Private result-source row directory owner.
use super::{BatchRow, QueryExecutionControls};
use crate::app::CassieError;
use crate::runtime::QueryMemoryReservation;

#[derive(Debug)]
pub(crate) struct OwnedRows {
    rows: Vec<BatchRow>,
    directory: Option<QueryMemoryReservation>,
}

#[cfg(test)]
pub(super) struct OwnedRowsIntoIter {
    rows: std::vec::IntoIter<BatchRow>,
    directory: QueryMemoryReservation,
}

impl OwnedRows {
    pub(crate) fn new(
        rows: Vec<BatchRow>,
        directory: QueryMemoryReservation,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let pending = Self {
            rows,
            directory: Some(directory),
        };
        if !pending
            .directory
            .as_ref()
            .expect("directory")
            .belongs_to(controls)
        {
            return Err(CassieError::ResourceLimit(
                "row directory uses a different query tracker".into(),
            ));
        }
        let bytes = pending
            .rows
            .capacity()
            .checked_mul(std::mem::size_of::<BatchRow>())
            .ok_or_else(|| CassieError::ResourceLimit("row directory capacity overflow".into()))?;
        if bytes > pending.directory.as_ref().expect("directory").bytes() {
            return Err(CassieError::ResourceLimit(
                "row directory exceeds its reservation".into(),
            ));
        }
        Ok(pending)
    }

    pub(crate) const fn untracked(rows: Vec<BatchRow>) -> Self {
        Self {
            rows,
            directory: None,
        }
    }
    pub(crate) fn into_legacy(self) -> Vec<BatchRow> {
        let Self { rows, directory } = self;
        drop(directory);
        rows
    }
    pub(crate) const fn is_tracked(&self) -> bool {
        self.directory.is_some()
    }
    #[cfg(test)]
    pub(crate) fn directory_bytes(&self) -> usize {
        self.directory
            .as_ref()
            .map_or(0, QueryMemoryReservation::bytes)
    }
    pub(crate) fn check_result(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if let Some(directory) = &self.directory {
            if !directory.belongs_to(controls) {
                return Err(CassieError::ResourceLimit(
                    "row directory uses a different query tracker".into(),
                ));
            }
            result_observer(controls, directory.bytes());
            controls.check_interruption()?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn into_iter(self) -> OwnedRowsIntoIter {
        let Self { rows, directory } = self;
        OwnedRowsIntoIter {
            rows: rows.into_iter(),
            directory: directory.expect("test directory"),
        }
    }
}

#[cfg(test)]
impl OwnedRowsIntoIter {
    pub(super) fn next_observing(
        &mut self,
        controls: &QueryExecutionControls,
        after_move: impl FnOnce(),
    ) -> Result<Option<BatchRow>, CassieError> {
        if !self.directory.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(
                "row iterator uses a different query tracker".into(),
            ));
        }
        controls.check_interruption()?;
        let row = self.rows.next();
        after_move();
        controls.check_interruption()?;
        Ok(row)
    }
}

impl std::ops::Deref for OwnedRows {
    type Target = [BatchRow];
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}
#[cfg(test)]
thread_local! {
    static CANCEL_RESULT: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static OBSERVE_RESULT: std::cell::RefCell<Option<(std::rc::Rc<std::cell::Cell<usize>>, std::rc::Rc<std::cell::Cell<usize>>)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn cancel_result_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_RESULT.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
pub(super) fn observe_result_once(
    directory: std::rc::Rc<std::cell::Cell<usize>>,
    total: std::rc::Rc<std::cell::Cell<usize>>,
) {
    OBSERVE_RESULT.with(|slot| *slot.borrow_mut() = Some((directory, total)));
}
#[cfg(test)]
fn result_observer(controls: &QueryExecutionControls, bytes: usize) {
    OBSERVE_RESULT.with(|slot| {
        if let Some((directory, total)) = slot.borrow_mut().take() {
            directory.set(bytes);
            total.set(controls.current_query_memory_bytes());
        }
    });
    CANCEL_RESULT.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn result_observer(_: &QueryExecutionControls, _: usize) {}
