//! Private implicit ownership over the existing session mutation stage.

use super::{CassieError, CassieSession};

impl CassieSession {
    pub(crate) fn begin_implicit_transaction(&self) -> Result<(), CassieError> {
        self.begin_transaction(None)?;
        self.transaction.lock().implicit = true;
        Ok(())
    }

    pub(crate) fn is_implicit_transaction(&self) -> bool {
        self.transaction.lock().implicit
    }
}
