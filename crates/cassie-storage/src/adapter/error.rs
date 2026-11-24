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
    #[error("invalid parameter value: {0}")]
    InvalidParameterValue(String),
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
    #[error("{message}")]
    Diagnostic {
        sql_state: &'static str,
        message: String,
        detail: Option<String>,
        hint: Option<String>,
    },
    #[error("engine policy callback failed: {0}")]
    EngineCallback(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl StorageError {
    #[must_use]
    pub fn diagnostic(sql_state: &'static str, message: impl Into<String>) -> Self {
        Self::Diagnostic {
            sql_state,
            message: message.into(),
            detail: None,
            hint: None,
        }
    }

    /// Returns the retry policy carried by a native or explicitly retryable error.
    #[must_use]
    pub fn storage_retry_kind(&self) -> Option<StorageRetryKind> {
        match self {
            Self::Native(cntryl_midge::MidgeError::WriteConflict(_)) => {
                Some(StorageRetryKind::WriteConflict)
            }
            Self::Native(cntryl_midge::MidgeError::WriteStall(_)) => {
                Some(StorageRetryKind::WriteStall)
            }
            Self::Native(cntryl_midge::MidgeError::Fenced(_)) => {
                Some(StorageRetryKind::Fenced)
            }
            Self::StorageRetryable(error) => Some(error.kind()),
            _ => None,
        }
    }
}

impl From<cassie_core::request::accounted::AccountingError> for StorageError {
    fn from(error: cassie_core::request::accounted::AccountingError) -> Self {
        Self::ResourceLimit(error.to_string())
    }
}

impl From<cassie_sql::definition_guard::DefinitionError> for StorageError {
    fn from(error: cassie_sql::definition_guard::DefinitionError) -> Self {
        match error {
            cassie_sql::definition_guard::DefinitionError::Unsupported(message) => {
                Self::Unsupported(message)
            }
            cassie_sql::definition_guard::DefinitionError::Execution(message) => {
                Self::Execution(message)
            }
        }
    }
}

impl From<crate::row_blob::RowBlobDecodeError<StorageError>> for StorageError {
    fn from(error: crate::row_blob::RowBlobDecodeError<StorageError>) -> Self {
        match error {
            crate::row_blob::RowBlobDecodeError::Codec(error) => Self::Codec(error),
            crate::row_blob::RowBlobDecodeError::Json(error) => error,
            crate::row_blob::RowBlobDecodeError::DataCorruption { sqlstate, message } => {
                Self::diagnostic(sqlstate, message)
            }
        }
    }
}

impl From<crate::row_blob::RowBlobValueError> for StorageError {
    fn from(error: crate::row_blob::RowBlobValueError) -> Self {
        match error {
            crate::row_blob::RowBlobValueError::InvalidValue(message) => {
                Self::InvalidVector(message)
            }
            crate::row_blob::RowBlobValueError::InvalidEncoding(message) => Self::Parse(message),
            crate::row_blob::RowBlobValueError::Unsupported(message) => Self::Unsupported(message),
            crate::row_blob::RowBlobValueError::TypeInput { sqlstate, message } => {
                Self::diagnostic(sqlstate, message)
            }
        }
    }
}

impl<E> From<crate::row_blob::RowBlobEncodeError<E>> for StorageError
where
    E: Into<StorageError>,
{
    fn from(error: crate::row_blob::RowBlobEncodeError<E>) -> Self {
        match error {
            crate::row_blob::RowBlobEncodeError::Layout(error) => Self::Codec(error),
            crate::row_blob::RowBlobEncodeError::FieldValue(error) => error.into(),
        }
    }
}

/// Decode one durable row through the Engine's scoped JSON parser.
///
/// Storage owns the persisted row framing, while the callback preserves the
/// Engine's query accounting and diagnostic policy for JSON values.
pub(crate) fn decode_row_with_hooks(
    schema: &crate::row_blob::RowSchema,
    row: &[u8],
    hooks: &super::StorageValueHooks,
) -> Result<serde_json::Value, StorageError> {
    crate::row_blob::decode_row(schema, row, hooks.decode_json).map_err(StorageError::from)
}

pub(crate) fn decode_projected_row_with_hooks(
    schema: &crate::row_blob::RowSchema,
    row: &[u8],
    projection: &std::collections::HashSet<String>,
    hooks: &super::StorageValueHooks,
) -> Result<serde_json::Value, StorageError> {
    crate::row_blob::decode_projected_row(schema, row, projection, hooks.decode_json)
        .map_err(StorageError::from)
}

pub(crate) fn decode_projected_row_with_aliases_and_hooks(
    schema: &crate::row_blob::RowSchema,
    row: &[u8],
    projection: &std::collections::HashSet<String>,
    hooks: &super::StorageValueHooks,
) -> Result<serde_json::Value, StorageError> {
    crate::row_blob::decode_projected_row_with_aliases(
        schema,
        row,
        projection,
        hooks.decode_json,
    )
    .map_err(StorageError::from)
}

pub(crate) fn decode_projected_row_matching_with_aliases_and_hooks(
    schema: &crate::row_blob::RowSchema,
    row: &[u8],
    projection: &std::collections::HashSet<String>,
    filter_field: &str,
    filter_value: &serde_json::Value,
    include_historical_aliases: bool,
    hooks: &super::StorageValueHooks,
) -> Result<Option<serde_json::Value>, StorageError> {
    crate::row_blob::decode_projected_row_matching_with_aliases(
        schema,
        row,
        projection,
        filter_field,
        filter_value,
        include_historical_aliases,
        hooks.decode_json,
    )
    .map_err(StorageError::from)
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

#[cfg(test)]
mod tests {
    use super::{StorageError, StorageRetryKind};

    #[test]
    fn should_preserve_native_retry_classification_at_the_storage_boundary() {
        // Arrange
        let errors = [
            (
                StorageError::from(cntryl_midge::MidgeError::WriteConflict("race".into())),
                StorageRetryKind::WriteConflict,
            ),
            (
                StorageError::from(cntryl_midge::MidgeError::WriteStall("full".into())),
                StorageRetryKind::WriteStall,
            ),
            (
                StorageError::from(cntryl_midge::MidgeError::Fenced("stale".into())),
                StorageRetryKind::Fenced,
            ),
        ];

        // Act
        let kinds = errors
            .iter()
            .map(|(error, _)| error.storage_retry_kind())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            kinds,
            errors
                .iter()
                .map(|(_, expected)| Some(*expected))
                .collect::<Vec<_>>()
        );
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
