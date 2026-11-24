//! Private implicit ownership over the existing session mutation stage.

#[cfg(test)]
#[path = "transaction_origin/tests.rs"]
mod tests;

use super::{CassieError, CassieSession};
use crate::sql::ast::TransactionModes;

impl CassieSession {
    #[cfg(test)]
    #[doc(hidden)]
    pub fn begin_implicit_transaction(&self) -> Result<(), CassieError> {
        self.begin_implicit_transaction_for_operation(None)
    }

    #[doc(hidden)]
    pub fn begin_implicit_transaction_for_operation(
        &self,
        authority: Option<&super::FailureAuthority>,
    ) -> Result<(), CassieError> {
        self.begin_transaction_with_origin_for_operation(
            None,
            TransactionModes::default(),
            true,
            authority,
        )?;
        #[cfg(test)]
        tests::pause_after_implicit_origin_publication();
        Ok(())
    }

    #[doc(hidden)]
    pub fn is_implicit_transaction(&self) -> bool {
        self.transaction.lock().implicit
    }
}
