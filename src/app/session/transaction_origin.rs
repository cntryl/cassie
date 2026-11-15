//! Private implicit ownership over the existing session mutation stage.

#[cfg(test)]
#[path = "transaction_origin/tests.rs"]
mod tests;

use super::{CassieError, CassieSession};
use crate::sql::ast::TransactionModes;

impl CassieSession {
    pub(crate) fn begin_implicit_transaction(&self) -> Result<(), CassieError> {
        self.begin_transaction_with_origin(None, TransactionModes::default(), true)?;
        #[cfg(test)]
        tests::pause_after_implicit_origin_publication();
        Ok(())
    }

    pub(crate) fn is_implicit_transaction(&self) -> bool {
        self.transaction.lock().implicit
    }
}
