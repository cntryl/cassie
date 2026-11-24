//! Engine compatibility facade for the storage-owned Midge adapter.

pub use cassie_storage::adapter::*;
pub(crate) use crate::app::{StatementDataRead, StatementReadScope};

#[cfg(test)]
pub(crate) mod statement_read {
    #[path = "adapter/statement_read/tests.rs"]
    mod tests;
}

impl From<cassie_storage::StorageError> for crate::app::CassieError {
    fn from(error: cassie_storage::StorageError) -> Self {
        use cassie_storage::StorageError;

        match error {
            StorageError::Native(error) => Self::from(error),
            StorageError::Codec(error) => Self::from(error),
            StorageError::Parse(message) => Self::Parse(message),
            StorageError::Execution(message) => Self::Execution(message),
            StorageError::InvalidQuery(message) => Self::InvalidQuery(message),
            StorageError::Unsupported(message) => Self::Unsupported(message),
            StorageError::InvalidVector(message) => Self::InvalidVector(message),
            StorageError::ResourceLimit(message) => Self::ResourceLimit(message),
            StorageError::QueryCanceled => Self::QueryCanceled,
            StorageError::DeadlineExceeded => Self::DeadlineExceeded,
            StorageError::CollectionNotFound(name) => Self::CollectionNotFound(name),
            StorageError::NotFound(message) => Self::NotFound(message),
            StorageError::Storage(message) => Self::Storage(message),
            StorageError::StorageBootstrap(message) => Self::StorageBootstrap(message),
            StorageError::StorageMissingFamily(message) => Self::StorageMissingFamily(message),
            StorageError::OutcomeUnknown(message) | StorageError::RecoveryRequired(message) => {
                Self::Storage(message)
            }
            StorageError::StorageRetryable(error) => Self::StorageRetryable(
                crate::app::RetryableStorageError::new(error.kind().into(), error.to_string()),
            ),
            StorageError::CatalogObjectNotFound { kind, name } => {
                Self::CatalogObjectNotFound {
                    kind: kind.into(),
                    name,
                }
            }
            StorageError::CatalogObjectAlreadyExists { kind, name } => {
                Self::CatalogObjectAlreadyExists {
                    kind: kind.into(),
                    name,
                }
            }
            StorageError::UniqueViolation {
                table,
                column,
                constraint,
            } => Self::UniqueViolation {
                table,
                column,
                constraint,
            },
            StorageError::Transaction { kind, message } => Self::Transaction {
                kind: kind.into(),
                message,
            },
        }
    }
}

impl From<cassie_storage::adapter::error::CatalogObjectKind> for crate::app::CatalogObjectKind {
    fn from(kind: cassie_storage::adapter::error::CatalogObjectKind) -> Self {
        match kind {
            cassie_storage::adapter::error::CatalogObjectKind::Database => Self::Database,
            cassie_storage::adapter::error::CatalogObjectKind::Relation => Self::Relation,
            cassie_storage::adapter::error::CatalogObjectKind::Schema => Self::Schema,
            cassie_storage::adapter::error::CatalogObjectKind::Index => Self::Index,
            cassie_storage::adapter::error::CatalogObjectKind::View => Self::View,
            cassie_storage::adapter::error::CatalogObjectKind::Role => Self::Role,
            cassie_storage::adapter::error::CatalogObjectKind::Sequence => Self::Sequence,
            cassie_storage::adapter::error::CatalogObjectKind::Rollup => Self::Rollup,
            cassie_storage::adapter::error::CatalogObjectKind::RetentionPolicy => {
                Self::RetentionPolicy
            }
            cassie_storage::adapter::error::CatalogObjectKind::ProjectionVersion => {
                Self::ProjectionVersion
            }
        }
    }
}

impl From<cassie_storage::adapter::error::TransactionErrorKind>
    for crate::app::TransactionErrorKind
{
    fn from(kind: cassie_storage::adapter::error::TransactionErrorKind) -> Self {
        match kind {
            cassie_storage::adapter::error::TransactionErrorKind::InvalidState => {
                Self::InvalidState
            }
            cassie_storage::adapter::error::TransactionErrorKind::ActiveTransaction => {
                Self::ActiveTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::NoActiveTransaction => {
                Self::NoActiveTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::FailedTransaction => {
                Self::FailedTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::ReadOnlyTransaction => {
                Self::ReadOnlyTransaction
            }
            cassie_storage::adapter::error::TransactionErrorKind::InvalidSavepoint => {
                Self::InvalidSavepoint
            }
            cassie_storage::adapter::error::TransactionErrorKind::SerializationFailure => {
                Self::SerializationFailure
            }
            cassie_storage::adapter::error::TransactionErrorKind::DeadlockDetected => {
                Self::DeadlockDetected
            }
            cassie_storage::adapter::error::TransactionErrorKind::LockNotAvailable => {
                Self::LockNotAvailable
            }
        }
    }
}

impl From<cassie_storage::adapter::error::StorageRetryKind> for crate::app::StorageRetryKind {
    fn from(kind: cassie_storage::adapter::error::StorageRetryKind) -> Self {
        match kind {
            cassie_storage::adapter::error::StorageRetryKind::WriteConflict => Self::WriteConflict,
            cassie_storage::adapter::error::StorageRetryKind::WriteStall => Self::WriteStall,
            cassie_storage::adapter::error::StorageRetryKind::Fenced => Self::Fenced,
            cassie_storage::adapter::error::StorageRetryKind::Unavailable => Self::Unavailable,
        }
    }
}
