//! Native writable handles cannot bypass a tracked row-history authority.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use cntryl_midge::ColumnFamilyId;
use parking_lot::Mutex;

use super::CassieError;

#[derive(Default)]
pub(super) struct NativeAccess {
    state: Mutex<AccessState>,
}

#[derive(Default)]
struct AccessState {
    native_writer_families: HashSet<ColumnFamilyId>,
    tracked_write_admitted: bool,
}

impl NativeAccess {
    pub(super) fn open_writable<T>(
        &self,
        family: ColumnFamilyId,
        retiring: &AtomicBool,
        open: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let mut state = self.state.lock();
        if retiring.load(Ordering::Acquire) {
            return Err(CassieError::Unsupported(
                "native writable transactions are unavailable while the database family is retiring".to_owned(),
            ));
        }
        if state.tracked_write_admitted {
            return Err(CassieError::Unsupported(
                "native writable handles are unavailable after tracked REPEATABLE READ writes were admitted; close and reopen this storage instance to change interop mode".to_owned(),
            ));
        }
        let opened = open()?;
        state.native_writer_families.insert(family);
        Ok(opened)
    }

    pub(super) fn begin_retirement(
        &self,
        family: ColumnFamilyId,
        retiring: &AtomicBool,
        database: &str,
    ) -> Result<(), CassieError> {
        let state = self.state.lock();
        if state.native_writer_families.contains(&family) {
            return Err(CassieError::Unsupported(format!(
                "cannot drop database '{database}' after a native writable transaction was issued; close its handles and reopen storage before retrying"
            )));
        }
        retiring.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) fn tracked<T>(
        &self,
        register: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let mut state = self.state.lock();
        if !state.native_writer_families.is_empty() {
            return Err(CassieError::Unsupported(
                "REPEATABLE READ writes are unavailable after a native writable handle was issued; close and reopen this storage instance to change interop mode".to_owned(),
            ));
        }
        let registered = register()?;
        state.tracked_write_admitted = true;
        Ok(registered)
    }
}

#[cfg(test)]
mod tests;
