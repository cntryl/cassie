#[path = "error/execution.rs"]
mod execution;
use super::{EmbeddingError, QueryError, TransactionErrorKind};
use crate::catalog::local_name;
use execution::execution_descriptor;

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

impl CatalogObjectKind {
    const fn label(self) -> &'static str {
        match self {
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
        }
    }

    const fn sql_state(self) -> &'static str {
        match self {
            Self::Database => "3D000",
            Self::Relation | Self::View | Self::Sequence => "42P01",
            Self::Schema => "3F000",
            Self::Index
            | Self::Role
            | Self::Rollup
            | Self::RetentionPolicy
            | Self::ProjectionVersion => "42704",
        }
    }
}

impl std::fmt::Display for CatalogObjectKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CassieErrorDescriptor {
    pub(crate) http_status: u16,
    pub(crate) sql_state: &'static str,
    pub(crate) message: String,
    pub(crate) table: Option<String>,
    pub(crate) column: Option<String>,
    pub(crate) constraint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CassieError {
    #[error("unresolvable column reference '{0}'; known fields or aliases required")]
    UndefinedColumn(String),

    #[error("column reference '{0}' is ambiguous")]
    AmbiguousColumn(String),

    #[error(transparent)]
    BooleanInput(#[from] crate::types::BooleanInputError),

    #[error(transparent)]
    FloatInput(#[from] crate::types::FloatInputError),

    #[error(transparent)]
    IntegerBytea(#[from] crate::types::IntegerByteaError),

    #[error(transparent)]
    ByteaInput(#[from] crate::types::ByteaInputError),

    #[error("collection not found: {0}")]
    CollectionNotFound(String),

    #[error(
        "field '{column}' cannot be null (null value in column '{column}' of relation '{table}' violates not-null constraint)"
    )]
    NotNullViolation {
        table: String,
        column: String,
        constraint: Option<String>,
    },

    #[error(
        "unique constraint failed for '{column}' (duplicate key value violates unique constraint '{constraint}')"
    )]
    UniqueViolation {
        table: String,
        column: String,
        constraint: String,
    },

    #[error(
        "check constraint failed for '{column}' field (new row for relation '{table}' violates check constraint '{constraint}')"
    )]
    CheckViolation {
        table: String,
        column: String,
        constraint: String,
    },

    #[error("insert or update on table '{table}' violates foreign key constraint '{constraint}'")]
    ForeignKeyViolation {
        table: String,
        column: String,
        constraint: String,
        referenced_table: String,
        referenced_column: String,
    },

    #[error("{0}")]
    CardinalityViolation(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("invalid query: {0}")]
    InvalidQuery(String),

    #[error("planner error: {0}")]
    Planner(String),

    #[error("execution error: {0}")]
    Execution(String),

    /// A typed transaction or lock failure shared by embedded, wire and REST paths.
    #[error("{message}")]
    Transaction {
        kind: TransactionErrorKind,
        message: String,
    },

    #[error("query timeout exceeded")]
    DeadlineExceeded,

    #[error("query canceled")]
    QueryCancelled,

    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),

    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("invalid vector: {0}")]
    InvalidVector(String),

    #[error("invalid embedding: {0}")]
    InvalidEmbedding(String),

    #[error("{0}")]
    ArrayInput(String),

    #[error("null array element not allowed in this context")]
    NullArrayElement,

    #[error(transparent)]
    SizeInput(#[from] crate::types::SizeInputError),

    #[error("invalid parameter value: {0}")]
    InvalidParameterValue(String),

    #[error("embedding unavailable: {0}")]
    EmbeddingUnavailable(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("authentication rate limit exceeded")]
    AuthenticationRateLimited,

    #[error("insufficient privilege")]
    InsufficientPrivilege,

    #[error("{kind} '{name}' does not exist")]
    CatalogObjectNotFound {
        kind: CatalogObjectKind,
        name: String,
    },

    #[error("{kind} '{name}' already exists")]
    CatalogObjectAlreadyExists {
        kind: CatalogObjectKind,
        name: String,
    },

    #[error("relation reference is ambiguous: {0}")]
    AmbiguousRelation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("unsupported feature: {0}")]
    Unsupported(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("storage bootstrap error: {0}")]
    StorageBootstrap(String),

    #[error("storage family missing: {0}")]
    StorageMissingFamily(String),

    #[error("temporary storage unavailable: {0}")]
    StorageRetryable(RetryableStorageError),
}

impl CassieError {
    pub(crate) fn descriptor(&self) -> CassieErrorDescriptor {
        match self {
            Self::UndefinedColumn(_) => bad_request_descriptor("42703", self.to_string()),
            Self::AmbiguousColumn(_) => bad_request_descriptor("42702", self.to_string()),
            Self::BooleanInput(error) => bad_request_descriptor("22P02", error.to_string()),
            Self::FloatInput(error) => bad_request_descriptor(error.sql_state(), error.to_string()),
            Self::IntegerBytea(error) => bad_request_descriptor("22003", error.to_string()),
            Self::ByteaInput(error) => bad_request_descriptor(error.sql_state(), error.to_string()),
            Self::CollectionNotFound(table) => missing_relation_descriptor(table),
            Self::CatalogObjectNotFound { kind, name } => {
                missing_catalog_object_descriptor(*kind, name, self.to_string())
            }
            Self::CatalogObjectAlreadyExists { kind, name } => {
                existing_catalog_object_descriptor(*kind, name, self.to_string())
            }
            Self::NotNullViolation { .. }
            | Self::UniqueViolation { .. }
            | Self::CheckViolation { .. }
            | Self::ForeignKeyViolation { .. } => constraint_descriptor(self),
            Self::InvalidQuery(message) if message == "invalid Unicode escape in escape string" => {
                bad_request_descriptor("22025", "invalid Unicode escape".into())
            }
            Self::InvalidQuery(message)
                if message.starts_with("invalid byte sequence for encoding \"UTF8\":") =>
            {
                bad_request_descriptor("22021", message.clone())
            }
            Self::Parse(message) | Self::InvalidQuery(message) => {
                bad_request_descriptor("42601", message.clone())
            }
            Self::Planner(message) => planner_descriptor(message),
            Self::Transaction { kind, message } => CassieErrorDescriptor {
                http_status: kind.http_status(),
                sql_state: kind.sql_state(),
                message: message.clone(),
                table: None,
                column: None,
                constraint: None,
            },
            Self::DeadlineExceeded => timeout_descriptor(),
            Self::QueryCancelled => cancellation_descriptor(),
            Self::ResourceLimit(message) => resource_limit_descriptor(message.clone()),
            Self::Execution(message) => execution_descriptor(message),
            Self::Configuration(_) => service_unavailable_descriptor("58030", self.to_string()),
            Self::InvalidVector(_) | Self::InvalidEmbedding(_) => {
                bad_request_descriptor("22000", self.to_string())
            }
            Self::ArrayInput(message) => bad_request_descriptor("22P02", message.clone()),
            Self::NullArrayElement => bad_request_descriptor("22004", self.to_string()),
            Self::SizeInput(error) => bad_request_descriptor(error.sql_state(), error.to_string()),
            Self::InvalidParameterValue(message) => {
                bad_request_descriptor("22023", message.clone())
            }
            Self::CardinalityViolation(message) => bad_request_descriptor("21000", message.clone()),
            Self::EmbeddingUnavailable(_)
            | Self::Storage(_)
            | Self::StorageBootstrap(_)
            | Self::StorageMissingFamily(_) => {
                service_unavailable_descriptor("58030", self.to_string())
            }
            Self::Unauthorized => unauthorized_descriptor(),
            Self::AuthenticationRateLimited => authentication_rate_limited_descriptor(),
            Self::InsufficientPrivilege => insufficient_privilege_descriptor(),
            Self::AmbiguousRelation(message) => bad_request_descriptor("42P09", message.clone()),
            Self::NotFound(_) => not_found_descriptor(self.to_string()),
            Self::Unsupported(_) => unsupported_descriptor(self.to_string()),
            Self::StorageRetryable(_) => service_unavailable_descriptor("57P03", self.to_string()),
        }
    }
}

fn existing_catalog_object_descriptor(
    kind: CatalogObjectKind,
    name: &str,
    message: String,
) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 409,
        sql_state: match kind {
            CatalogObjectKind::Database => "42P04",
            _ => "42710",
        },
        message,
        table: matches!(kind, CatalogObjectKind::Relation | CatalogObjectKind::View)
            .then(|| local_name(name)),
        column: None,
        constraint: None,
    }
}

fn missing_relation_descriptor(table: &str) -> CassieErrorDescriptor {
    let table = local_name(table);
    CassieErrorDescriptor {
        http_status: 404,
        sql_state: "42P01",
        message: format!("collection not found: {table}"),
        table: Some(table),
        column: None,
        constraint: None,
    }
}

fn missing_catalog_object_descriptor(
    kind: CatalogObjectKind,
    name: &str,
    message: String,
) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 404,
        sql_state: kind.sql_state(),
        message,
        table: matches!(kind, CatalogObjectKind::Relation | CatalogObjectKind::View)
            .then(|| local_name(name)),
        column: None,
        constraint: None,
    }
}

fn constraint_descriptor(error: &CassieError) -> CassieErrorDescriptor {
    match error {
        CassieError::NotNullViolation {
            table,
            column,
            constraint,
        } => conflict_descriptor(
            "23502",
            table,
            column,
            constraint.clone(),
            error.to_string(),
        ),
        CassieError::UniqueViolation {
            table,
            column,
            constraint,
        } => conflict_descriptor(
            "23505",
            table,
            column,
            Some(constraint.clone()),
            error.to_string(),
        ),
        CassieError::CheckViolation {
            table,
            column,
            constraint,
        } => conflict_descriptor(
            "23514",
            table,
            column,
            Some(constraint.clone()),
            error.to_string(),
        ),
        CassieError::ForeignKeyViolation {
            table,
            column,
            constraint,
            ..
        } => conflict_descriptor(
            "23503",
            table,
            column,
            Some(constraint.clone()),
            error.to_string(),
        ),
        _ => unreachable!("constraint descriptor only handles constraint errors"),
    }
}

fn conflict_descriptor(
    sql_state: &'static str,
    table: &str,
    column: &str,
    constraint: Option<String>,
    message: String,
) -> CassieErrorDescriptor {
    let table = local_name(table);
    CassieErrorDescriptor {
        http_status: 409,
        sql_state,
        message,
        table: Some(table),
        column: Some(column.to_string()),
        constraint,
    }
}

fn bad_request_descriptor(sql_state: &'static str, message: String) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 400,
        sql_state,
        message,
        table: None,
        column: None,
        constraint: None,
    }
}

fn service_unavailable_descriptor(
    sql_state: &'static str,
    message: String,
) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 503,
        sql_state,
        message,
        table: None,
        column: None,
        constraint: None,
    }
}

fn timeout_descriptor() -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 504,
        sql_state: "57014",
        message: CassieError::DeadlineExceeded.to_string(),
        table: None,
        column: None,
        constraint: None,
    }
}

fn cancellation_descriptor() -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 499,
        sql_state: "57014",
        message: CassieError::QueryCancelled.to_string(),
        table: None,
        column: None,
        constraint: None,
    }
}

fn resource_limit_descriptor(message: String) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 400,
        sql_state: if message == "ROW expressions can have at most 1664 entries" {
            "54011"
        } else {
            "54000"
        },
        message,
        table: None,
        column: None,
        constraint: None,
    }
}

fn unauthorized_descriptor() -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 401,
        sql_state: "28000",
        message: CassieError::Unauthorized.to_string(),
        table: None,
        column: None,
        constraint: None,
    }
}

fn authentication_rate_limited_descriptor() -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 429,
        sql_state: "53300",
        message: CassieError::AuthenticationRateLimited.to_string(),
        table: None,
        column: None,
        constraint: None,
    }
}

fn insufficient_privilege_descriptor() -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 403,
        sql_state: "42501",
        message: CassieError::InsufficientPrivilege.to_string(),
        table: None,
        column: None,
        constraint: None,
    }
}

fn not_found_descriptor(message: String) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 404,
        sql_state: "02000",
        message,
        table: None,
        column: None,
        constraint: None,
    }
}

fn unsupported_descriptor(message: String) -> CassieErrorDescriptor {
    CassieErrorDescriptor {
        http_status: 501,
        sql_state: "0A000",
        message,
        table: None,
        column: None,
        constraint: None,
    }
}

fn planner_descriptor(message: &str) -> CassieErrorDescriptor {
    if message == "relation alias list has more names than source columns" {
        return bad_request_descriptor("42P10", message.to_string());
    }
    if message.eq_ignore_ascii_case("query timeout exceeded") {
        return timeout_descriptor();
    }
    bad_request_descriptor("42601", message.to_string())
}

fn pagination_sql_state(message: &str) -> Option<&'static str> {
    match message {
        "LIMIT must not be negative" => Some("2201W"),
        "OFFSET must not be negative" => Some("2201X"),
        "pagination operator is ambiguous for unknown parameters" => Some("42725"),
        "pagination requires an admitted integer expression" => Some("42804"),
        "argument of LIMIT must not contain variables"
        | "argument of OFFSET must not contain variables" => Some("42P10"),
        _ => None,
    }
}

pub(crate) fn unsupported_sql_error(sql: &str) -> Option<CassieError> {
    let keyword = sql.split_whitespace().next()?;
    let keyword = keyword.trim_matches(|character: char| !character.is_ascii_alphabetic());
    let keyword = keyword.to_ascii_uppercase();

    match keyword.as_str() {
        "COPY" | "LISTEN" | "NOTIFY" | "UNLISTEN" => Some(CassieError::Unsupported(format!(
            "{keyword} is not supported"
        ))),
        _ => None,
    }
}

impl From<QueryError> for CassieError {
    fn from(value: QueryError) -> Self {
        match value {
            QueryError::General(message)
                if message.eq_ignore_ascii_case("query timeout exceeded") =>
            {
                CassieError::DeadlineExceeded
            }
            QueryError::General(message) if message.eq_ignore_ascii_case("query canceled") => {
                CassieError::QueryCancelled
            }
            QueryError::General(message)
                if message.starts_with("query memory budget exceeded:") =>
            {
                CassieError::ResourceLimit(message)
            }
            QueryError::General(message)
                if message.starts_with("query result row limit exceeded:") =>
            {
                CassieError::ResourceLimit(message)
            }
            QueryError::General(message) => CassieError::Execution(message),
            QueryError::Cassie(error) => error,
        }
    }
}

impl From<crate::config::CassieRuntimeConfigError> for CassieError {
    fn from(value: crate::config::CassieRuntimeConfigError) -> Self {
        CassieError::Configuration(value.to_string())
    }
}

impl From<EmbeddingError> for CassieError {
    fn from(value: EmbeddingError) -> Self {
        match value {
            EmbeddingError::Cancelled { .. } => CassieError::QueryCancelled,
            EmbeddingError::InvalidConfiguration(message) | EmbeddingError::ParseError(message) => {
                CassieError::InvalidEmbedding(message)
            }
            EmbeddingError::Unavailable { provider, reason } => {
                CassieError::EmbeddingUnavailable(format!("{provider}: {reason}"))
            }
            EmbeddingError::NotImplemented { provider } => CassieError::EmbeddingUnavailable(
                format!("embedding provider '{provider}' is not implemented"),
            ),
            EmbeddingError::Timeout { provider, message } => {
                CassieError::EmbeddingUnavailable(format!("{provider}: {message}"))
            }
            EmbeddingError::RetryExhausted {
                provider,
                attempts,
                message,
            } => CassieError::EmbeddingUnavailable(format!(
                "{provider}: exhausted retry attempts ({attempts}) after: {message}"
            )),
            EmbeddingError::RequestError(message) => CassieError::EmbeddingUnavailable(message),
            EmbeddingError::ResponseTooLarge {
                provider,
                limit_bytes,
            } => CassieError::EmbeddingUnavailable(format!(
                "{provider}: response exceeds {limit_bytes} bytes"
            )),
        }
    }
}

impl From<std::string::FromUtf8Error> for CassieError {
    fn from(value: std::string::FromUtf8Error) -> Self {
        CassieError::Parse(value.to_string())
    }
}

impl From<crate::sql::SqlError> for CassieError {
    fn from(value: crate::sql::SqlError) -> Self {
        match value.kind() {
            crate::sql::SqlErrorKind::Syntax => CassieError::InvalidQuery(value.to_string()),
            crate::sql::SqlErrorKind::NumericRange => CassieError::Execution(value.to_string()),
            crate::sql::SqlErrorKind::Unsupported => CassieError::Unsupported(value.to_string()),
            crate::sql::SqlErrorKind::ResourceLimit => {
                CassieError::ResourceLimit(value.to_string())
            }
        }
    }
}

const STORAGE_WRITE_CONFLICT_PREFIX: &str = "midge write conflict: ";
const STORAGE_WRITE_STALL_PREFIX: &str = "midge write stalled: ";
const STORAGE_FENCED_PREFIX: &str = "midge fenced: ";

/// Kind of a [`CassieError::StorageRetryable`] storage failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageRetryKind {
    /// Optimistic transaction conflict with another writer.
    WriteConflict,
    /// Storage backpressure while memtables flush or compaction catches up.
    WriteStall,
    /// This writer's lease epoch is stale because another writer took over.
    Fenced,
    /// Any other temporary unavailability.
    Unavailable,
}

/// Payload of [`CassieError::StorageRetryable`]: the typed failure kind plus
/// its message. It dereferences to the message text.
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

impl std::ops::Deref for RetryableStorageError {
    type Target = str;

    fn deref(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for RetryableStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl CassieError {
    /// Returns the typed kind of a retryable storage error.
    pub(crate) fn storage_retry_kind(&self) -> Option<StorageRetryKind> {
        let Self::StorageRetryable(error) = self else {
            return None;
        };
        Some(error.kind())
    }

    fn storage_retryable(kind: StorageRetryKind, prefix: &str, message: &str) -> Self {
        Self::StorageRetryable(RetryableStorageError::new(
            kind,
            format!("{prefix}{message}"),
        ))
    }
}

impl From<cntryl_midge::MidgeError> for CassieError {
    fn from(value: cntryl_midge::MidgeError) -> Self {
        let message = value.to_string();
        match value {
            cntryl_midge::MidgeError::OutcomeUnknown(detail) => CassieError::Storage(format!(
                "midge write outcome unknown: {detail}; reconcile the transaction after recovery before retrying"
            )),
            cntryl_midge::MidgeError::WriteStall(message) => CassieError::storage_retryable(
                StorageRetryKind::WriteStall,
                STORAGE_WRITE_STALL_PREFIX,
                &message,
            ),
            cntryl_midge::MidgeError::Fenced(message) => CassieError::storage_retryable(
                StorageRetryKind::Fenced,
                STORAGE_FENCED_PREFIX,
                &message,
            ),
            cntryl_midge::MidgeError::NotFound => {
                CassieError::StorageMissingFamily("midge key not found".to_string())
            }
            cntryl_midge::MidgeError::InvalidArgument(message) => {
                if message.to_ascii_lowercase().contains("does not exist") {
                    CassieError::StorageMissingFamily(format!(
                        "midge family missing or invalid argument: {message}"
                    ))
                } else {
                    CassieError::Storage(message)
                }
            }
            cntryl_midge::MidgeError::WriteConflict(_) => {
                CassieError::storage_retryable(
                    StorageRetryKind::WriteConflict,
                    STORAGE_WRITE_CONFLICT_PREFIX,
                    &message,
                )
            }
            _ => CassieError::Storage(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_map_aggregate_integer_overflow_to_numeric_value_out_of_range() {
        // Arrange
        let error = CassieError::Execution("aggregate integer overflow".to_string());

        // Act
        let descriptor = error.descriptor();

        // Assert
        assert_eq!(descriptor.sql_state, "22003");
    }

    #[test]
    fn should_map_float_overflow_to_numeric_value_out_of_range() {
        // Arrange
        let error = CassieError::Execution(crate::types::numeric::FLOAT_OVERFLOW.to_string());

        // Act
        let descriptor = error.descriptor();

        // Assert
        assert_eq!(descriptor.sql_state, "22003");
    }

    #[test]
    fn should_map_non_numeric_aggregate_input_to_undefined_function() {
        // Arrange
        let error = CassieError::Execution(crate::types::numeric::non_numeric_aggregate_input(
            "SUM",
            &crate::types::Value::String("a".to_string()),
        ));

        // Act
        let descriptor = error.descriptor();

        // Assert
        assert_eq!(descriptor.sql_state, "42883");
        assert_eq!(descriptor.message, "function sum(text) does not exist");
    }

    #[test]
    fn should_classify_retryable_storage_errors_by_kind() {
        // Arrange
        let errors = [
            cntryl_midge::MidgeError::WriteConflict("overlap".to_string()),
            cntryl_midge::MidgeError::WriteStall("memtable full".to_string()),
            cntryl_midge::MidgeError::Fenced("stale epoch".to_string()),
        ];

        // Act
        let kinds = errors
            .into_iter()
            .map(|error| CassieError::from(error).storage_retry_kind())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            kinds,
            vec![
                Some(StorageRetryKind::WriteConflict),
                Some(StorageRetryKind::WriteStall),
                Some(StorageRetryKind::Fenced),
            ]
        );
        assert_eq!(
            CassieError::Storage("disk".to_string()).storage_retry_kind(),
            None
        );
        assert_eq!(
            CassieError::StorageRetryable("midge write conflict: spoofed".into())
                .storage_retry_kind(),
            Some(StorageRetryKind::Unavailable)
        );
    }

    #[test]
    fn should_map_transaction_error_kinds_independently_of_message_text() {
        // Arrange
        let kinds = [
            (TransactionErrorKind::InvalidState, "25000", 400),
            (TransactionErrorKind::ActiveTransaction, "25001", 400),
            (TransactionErrorKind::NoActiveTransaction, "25P01", 400),
            (TransactionErrorKind::FailedTransaction, "25P02", 400),
            (TransactionErrorKind::ReadOnlyTransaction, "25006", 400),
            (TransactionErrorKind::InvalidSavepoint, "3B001", 400),
            (TransactionErrorKind::SerializationFailure, "40001", 409),
            (TransactionErrorKind::DeadlockDetected, "40P01", 409),
            (TransactionErrorKind::LockNotAvailable, "55P03", 409),
        ];

        // Act
        let descriptors = kinds.map(|(kind, _, _)| {
            CassieError::Transaction {
                kind,
                message: "division by zero".to_string(),
            }
            .descriptor()
        });

        // Assert
        for ((_, sql_state, http_status), descriptor) in kinds.iter().zip(descriptors) {
            assert_eq!(descriptor.sql_state, *sql_state);
            assert_eq!(descriptor.http_status, *http_status);
            assert_eq!(descriptor.message, "division by zero");
        }
    }

    #[test]
    fn should_map_scalar_integer_overflow_to_numeric_value_out_of_range() {
        // Arrange
        let error = CassieError::Execution("bigint out of range".to_string());

        // Act
        let descriptor = error.descriptor();

        // Assert
        assert_eq!(descriptor.sql_state, "22003");
    }
}
