//! Preserve native mutation errors while adding typed Cassie admission failures.

use super::CassieError;

#[derive(Debug)]
pub(crate) enum AdapterMutationError {
    Native(cntryl_midge::MidgeError),
    Cassie(CassieError),
}

impl From<cntryl_midge::MidgeError> for AdapterMutationError {
    fn from(error: cntryl_midge::MidgeError) -> Self {
        Self::Native(error)
    }
}

impl From<CassieError> for AdapterMutationError {
    fn from(error: CassieError) -> Self {
        Self::Cassie(error)
    }
}

impl From<AdapterMutationError> for CassieError {
    fn from(error: AdapterMutationError) -> Self {
        match error {
            AdapterMutationError::Native(error) => Self::from(error),
            AdapterMutationError::Cassie(error) => error,
        }
    }
}
