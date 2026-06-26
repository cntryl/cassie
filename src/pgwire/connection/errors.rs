use crate::app::CassieError;

#[derive(Debug, Clone, Copy)]
pub(super) enum PgWireSeverity {
    Error,
    Fatal,
}

impl PgWireSeverity {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PgWireError {
    pub(super) severity: PgWireSeverity,
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) detail: Option<String>,
    pub(super) hint: Option<String>,
    pub(super) schema: Option<String>,
    pub(super) table: Option<String>,
    pub(super) column: Option<String>,
    pub(super) constraint: Option<String>,
}

impl PgWireError {
    pub(super) fn new(
        severity: PgWireSeverity,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            detail: None,
            hint: None,
            schema: None,
            table: None,
            column: None,
            constraint: None,
        }
    }

    pub(super) fn protocol(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "08P01", message)
    }

    pub(super) fn fatal_protocol(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Fatal, "08P01", message)
    }

    pub(super) fn auth_failed(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Fatal, "28000", message)
    }

    pub(super) fn auth_required(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "28000", message)
    }

    pub(super) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "0A000", message)
    }

    pub(super) fn invalid_statement(name: &str) -> Self {
        Self::new(
            PgWireSeverity::Error,
            "26000",
            format!("statement '{name}' is not prepared"),
        )
    }

    pub(super) fn invalid_portal(name: &str) -> Self {
        Self::new(
            PgWireSeverity::Error,
            "26000",
            format!("portal '{name}' is not bound"),
        )
    }

    pub(super) fn from_cassie_error(severity: PgWireSeverity, error: &CassieError) -> Self {
        match error {
            CassieError::Parse(message) => Self::new(severity, "42601", message.clone()),
            CassieError::Unauthorized => Self::new(severity, "28000", error.to_string()),
            CassieError::CollectionNotFound(table) => {
                let mut pg_error = Self::new(severity, "42P01", error.to_string());
                pg_error.table = Some(table.clone());
                pg_error
            }
            CassieError::NotNullViolation {
                table,
                column,
                constraint,
            } => {
                let mut pg_error = Self::new(severity, "23502", error.to_string());
                pg_error.table = Some(table.clone());
                pg_error.column = Some(column.clone());
                pg_error.constraint = constraint.clone();
                pg_error
            }
            CassieError::UniqueViolation {
                table,
                column,
                constraint,
            } => {
                let mut pg_error = Self::new(severity, "23505", error.to_string());
                pg_error.table = Some(table.clone());
                pg_error.column = Some(column.clone());
                pg_error.constraint = Some(constraint.clone());
                pg_error
            }
            CassieError::CheckViolation {
                table,
                column,
                constraint,
            } => {
                let mut pg_error = Self::new(severity, "23514", error.to_string());
                pg_error.table = Some(table.clone());
                pg_error.column = Some(column.clone());
                pg_error.constraint = Some(constraint.clone());
                pg_error
            }
            CassieError::ForeignKeyViolation {
                table,
                column,
                constraint,
                ..
            } => {
                let mut pg_error = Self::new(severity, "23503", error.to_string());
                pg_error.table = Some(table.clone());
                pg_error.column = Some(column.clone());
                pg_error.constraint = Some(constraint.clone());
                pg_error
            }
            CassieError::Unsupported(_) => Self::new(severity, "0A000", error.to_string()),
            CassieError::InvalidVector(_) | CassieError::InvalidEmbedding(_) => {
                Self::new(severity, "22000", error.to_string())
            }
            CassieError::EmbeddingUnavailable(_) => Self::new(severity, "58030", error.to_string()),
            CassieError::Storage(_)
            | CassieError::StorageBootstrap(_)
            | CassieError::StorageMissingFamily(_)
            | CassieError::StorageRetryable(_)
            | CassieError::Planner(_)
            | CassieError::Execution(_)
            | CassieError::Configuration(_)
            | CassieError::NotFound(_) => Self::new(severity, "XX000", error.to_string()),
        }
    }
}
