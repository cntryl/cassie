use super::{EmbeddingError, QueryError};
use crate::catalog::local_name;

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
        }
    }

    const fn sql_state(self) -> &'static str {
        match self {
            Self::Database => "3D000",
            Self::Relation | Self::View | Self::Sequence => "42P01",
            Self::Schema => "3F000",
            Self::Index | Self::Role | Self::Rollup | Self::RetentionPolicy => "42704",
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

    #[error("parse error: {0}")]
    Parse(String),

    #[error("invalid query: {0}")]
    InvalidQuery(String),

    #[error("planner error: {0}")]
    Planner(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("query timeout exceeded")]
    DeadlineExceeded,

    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("invalid vector: {0}")]
    InvalidVector(String),

    #[error("invalid embedding: {0}")]
    InvalidEmbedding(String),

    #[error("embedding unavailable: {0}")]
    EmbeddingUnavailable(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("insufficient privilege")]
    InsufficientPrivilege,

    #[error("{kind} '{name}' does not exist")]
    CatalogObjectNotFound {
        kind: CatalogObjectKind,
        name: String,
    },

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
    StorageRetryable(String),
}

impl CassieError {
    pub(crate) fn descriptor(&self) -> CassieErrorDescriptor {
        match self {
            Self::CollectionNotFound(table) => missing_relation_descriptor(table),
            Self::CatalogObjectNotFound { kind, name } => {
                missing_catalog_object_descriptor(*kind, name, self.to_string())
            }
            Self::NotNullViolation { .. }
            | Self::UniqueViolation { .. }
            | Self::CheckViolation { .. }
            | Self::ForeignKeyViolation { .. } => constraint_descriptor(self),
            Self::Parse(message) | Self::InvalidQuery(message) => {
                bad_request_descriptor("42601", message.clone())
            }
            Self::Planner(message) => planner_descriptor(message),
            Self::DeadlineExceeded => timeout_descriptor(),
            Self::Execution(message) => execution_descriptor(message),
            Self::Configuration(_) => service_unavailable_descriptor("58030", self.to_string()),
            Self::InvalidVector(_) | Self::InvalidEmbedding(_) => {
                bad_request_descriptor("22000", self.to_string())
            }
            Self::EmbeddingUnavailable(_)
            | Self::Storage(_)
            | Self::StorageBootstrap(_)
            | Self::StorageMissingFamily(_) => {
                service_unavailable_descriptor("58030", self.to_string())
            }
            Self::Unauthorized => unauthorized_descriptor(),
            Self::InsufficientPrivilege => insufficient_privilege_descriptor(),
            Self::NotFound(_) => not_found_descriptor(self.to_string()),
            Self::Unsupported(_) => unsupported_descriptor(self.to_string()),
            Self::StorageRetryable(_) => service_unavailable_descriptor("57P03", self.to_string()),
        }
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
    if let Some(descriptor) = legacy_catalog_not_found_descriptor(message) {
        return descriptor;
    }
    if message.eq_ignore_ascii_case("query timeout exceeded") {
        return timeout_descriptor();
    }
    bad_request_descriptor("42601", message.to_string())
}

fn execution_descriptor(message: &str) -> CassieErrorDescriptor {
    if message.eq_ignore_ascii_case("query timeout exceeded") {
        return timeout_descriptor();
    }
    if message.eq_ignore_ascii_case("division by zero") {
        return bad_request_descriptor("22012", message.to_string());
    }
    bad_request_descriptor("22000", message.to_string())
}

fn legacy_catalog_not_found_descriptor(message: &str) -> Option<CassieErrorDescriptor> {
    [
        ("namespace '", CatalogObjectKind::Schema),
        ("schema '", CatalogObjectKind::Schema),
        ("index '", CatalogObjectKind::Index),
        ("view '", CatalogObjectKind::View),
        ("role '", CatalogObjectKind::Role),
        ("sequence '", CatalogObjectKind::Sequence),
        ("rollup '", CatalogObjectKind::Rollup),
        ("retention policy '", CatalogObjectKind::RetentionPolicy),
        ("relation '", CatalogObjectKind::Relation),
        ("collection '", CatalogObjectKind::Relation),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| {
        if !message.starts_with(prefix) || !message.ends_with(" does not exist") {
            return None;
        }
        let name = message
            .strip_prefix(prefix)?
            .strip_suffix(" does not exist")?
            .to_string();
        Some(CassieError::CatalogObjectNotFound { kind, name }.descriptor())
    })
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
            crate::sql::SqlErrorKind::Unsupported => CassieError::Unsupported(value.to_string()),
        }
    }
}

impl From<cntryl_midge::MidgeError> for CassieError {
    fn from(value: cntryl_midge::MidgeError) -> Self {
        let message = value.to_string();
        match value {
            cntryl_midge::MidgeError::WriteStall(message) => {
                CassieError::StorageRetryable(format!("midge write stalled: {message}"))
            }
            cntryl_midge::MidgeError::Fenced(message) => {
                CassieError::StorageRetryable(format!("midge fenced: {message}"))
            }
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
            _ if message.to_ascii_lowercase().contains("write conflict") => {
                CassieError::StorageRetryable(format!("midge write conflict: {message}"))
            }
            _ => CassieError::Storage(message),
        }
    }
}
