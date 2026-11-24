//! Typed failures crossing the concrete storage adapter boundary.

use cntryl_midge::MidgeError;

use crate::StorageCodecError;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Native(#[from] MidgeError),
    #[error(transparent)]
    Codec(#[from] StorageCodecError),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("execution error: {0}")]
    Execution(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
    #[error("invalid vector: {0}")]
    InvalidVector(String),
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),
    #[error("query canceled")]
    QueryCanceled,
    #[error("query timeout exceeded")]
    DeadlineExceeded,
    #[error("collection not found: {0}")]
    CollectionNotFound(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("storage bootstrap error: {0}")]
    StorageBootstrap(String),
    #[error("storage family missing: {0}")]
    StorageMissingFamily(String),
    #[error("storage outcome may be unknown: {0}")]
    OutcomeUnknown(String),
    #[error("storage recovery is required: {0}")]
    RecoveryRequired(String),
    #[error("retryable storage failure: {0}")]
    StorageRetryable(RetryableStorageError),
    #[error("catalog object {kind} '{name}' does not exist")]
    CatalogObjectNotFound { kind: CatalogObjectKind, name: String },
    #[error("catalog object {kind} '{name}' already exists")]
    CatalogObjectAlreadyExists { kind: CatalogObjectKind, name: String },
    #[error("unique constraint failed for '{column}'")]
    UniqueViolation {
        table: String,
        column: String,
        constraint: String,
    },
    #[error("transaction error: {message}")]
    Transaction {
        kind: TransactionErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogObjectKind {
    Database,
    Relation,
    Schema,
    Index,
    View,
    Role,
    Sequence,
    Rollup,
    RetentionPolicy,
    ProjectionVersion,
}

impl std::fmt::Display for CatalogObjectKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Database => "database",
            Self::Relation => "relation",
            Self::Schema => "schema",
            Self::Index => "index",
            Self::View => "view",
            Self::Role => "role",
            Self::Sequence => "sequence",
            Self::Rollup => "rollup",
            Self::RetentionPolicy => "retention policy",
            Self::ProjectionVersion => "projection version",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionErrorKind {
    InvalidState,
    ActiveTransaction,
    NoActiveTransaction,
    FailedTransaction,
    ReadOnlyTransaction,
    InvalidSavepoint,
    SerializationFailure,
    DeadlockDetected,
    LockNotAvailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageRetryKind {
    WriteConflict,
    WriteStall,
    Fenced,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryableStorageError {
    kind: StorageRetryKind,
    message: String,
}

impl RetryableStorageError {
    #[must_use]
    pub fn new(kind: StorageRetryKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> StorageRetryKind {
        self.kind
    }
}

impl std::fmt::Display for RetryableStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<String> for RetryableStorageError {
    fn from(message: String) -> Self {
        Self::new(StorageRetryKind::Unavailable, message)
    }
}

impl From<&str> for RetryableStorageError {
    fn from(message: &str) -> Self {
        Self::new(StorageRetryKind::Unavailable, message)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Parse(error.to_string())
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<cassie_core::request::accounted::AccountingError> for StorageError {
    fn from(error: cassie_core::request::accounted::AccountingError) -> Self {
        Self::ResourceLimit(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::StorageError;

    #[test]
    fn should_preserve_native_outcome_unknown_as_a_typed_storage_error() {
        // Arrange
        let native = cntryl_midge::MidgeError::OutcomeUnknown("commit status unavailable".into());

        // Act
        let error = StorageError::from(native);

        // Assert
        assert!(matches!(error, StorageError::Native(cntryl_midge::MidgeError::OutcomeUnknown(_))));
    }
}
