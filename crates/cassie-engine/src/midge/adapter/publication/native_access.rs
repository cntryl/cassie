//! Native writable handles cannot bypass a tracked row-history authority.

use parking_lot::Mutex;

use super::CassieError;

#[derive(Default)]
pub(super) struct NativeAccess {
    state: Mutex<AccessState>,
}

#[derive(Default)]
struct AccessState {
    native_writer_issued: bool,
    tracked_write_admitted: bool,
}

impl NativeAccess {
    pub(super) fn native<T>(
        &self,
        mode: cntryl_midge::TransactionMode,
        open: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        if mode == cntryl_midge::TransactionMode::ReadOnly {
            return open();
        }
        let mut state = self.state.lock();
        if state.tracked_write_admitted {
            return Err(CassieError::Unsupported(
                "native writable handles are unavailable after tracked REPEATABLE READ writes were admitted; close and reopen this storage instance to change interop mode".to_owned(),
            ));
        }
        let opened = open()?;
        state.native_writer_issued = true;
        Ok(opened)
    }

    pub(super) fn tracked<T>(
        &self,
        register: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let mut state = self.state.lock();
        if state.native_writer_issued {
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
