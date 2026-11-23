use crate::app::CassieError;

#[path = "errors/parameter_semantic.rs"]
mod parameter_semantic;

#[cfg(test)]
#[path = "errors/parameter_semantic_tests.rs"]
mod parameter_semantic_tests;

#[cfg(test)]
#[path = "errors/owned_row_frame_tests.rs"]
mod owned_row_frame_tests;

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

pub(super) fn cassie_pg_error(error: &CassieError) -> PgWireError {
    PgWireError::from_cassie_error(PgWireSeverity::Error, error)
}

pub(super) fn cassie_pg_error_with_sql(error: &CassieError, sql: &str) -> PgWireError {
    let mut mapped = cassie_pg_error(error);
    mapped.add_syntax_position(sql);
    mapped
}

#[derive(Debug)]
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
    pub(super) position: Option<u32>,
    // Owned strings and the admitted network frame precede their shared lease.
    semantic_frame: Option<parameter_semantic::AdmittedErrorFrame>,
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
            position: None,
            semantic_frame: None,
        }
    }

    pub(super) fn protocol(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "08P01", message)
    }

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "XX000", message)
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

    pub(super) fn add_syntax_position(&mut self, sql: &str) {
        if self.code != "42601" {
            return;
        }
        let Some((token, byte_offset)) = Self::syntax_error_token(sql) else {
            return;
        };
        self.message = format!("syntax error at or near \"{token}\"");
        self.position = Some(u32::try_from(sql[..byte_offset].chars().count() + 1).unwrap_or(1));
    }

    pub(super) fn too_many_connections() -> Self {
        Self::new(PgWireSeverity::Fatal, "53300", "too many connections")
    }

    fn syntax_error_token(sql: &str) -> Option<(&str, usize)> {
        let uppercase = sql.to_ascii_uppercase();
        let form = uppercase.find("FORM")?;
        let before = sql.as_bytes().get(form.wrapping_sub(1)).copied();
        let after_form = sql.as_bytes().get(form + 4).copied();
        if before.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || after_form.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return None;
        }
        let rest = sql.get(form + 4..)?;
        let leading = rest.len() - rest.trim_start().len();
        let token = rest
            .trim_start()
            .split(|character: char| {
                character.is_whitespace() || matches!(character, ';' | ',' | ')')
            })
            .next()?;
        (!token.is_empty()).then_some((token, form + 4 + leading))
    }

    pub(super) fn invalid_sql_statement_name(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "26000", message)
    }

    pub(super) fn invalid_cursor_name(message: impl Into<String>) -> Self {
        Self::new(PgWireSeverity::Error, "34000", message)
    }

    pub(super) fn from_cassie_error(severity: PgWireSeverity, error: &CassieError) -> Self {
        if let CassieError::OwnedNumericLiteral(diagnostic) = error {
            return match Self::controlled_diagnostic(
                severity,
                diagnostic.sqlstate(),
                diagnostic.message(),
                diagnostic.controls(),
            ) {
                Ok(mapped) => mapped,
                Err(error) => Self::from_cassie_error(severity, &error),
            };
        }
        let descriptor = error.descriptor();
        let mut pg_error = Self::new(severity, descriptor.sql_state, descriptor.message);
        if pg_error.code == "22023" && pg_error.message == "invalid base64 end sequence" {
            pg_error.hint = Some(
                "Input data is missing padding, is truncated, or is otherwise corrupted.".into(),
            );
        }
        if matches!(
            error,
            CassieError::Execution(message) | CassieError::InvalidQuery(message)
                if message == "invalid escape string"
        ) {
            pg_error.hint = Some("Escape string must be empty or one character.".into());
        }
        if let CassieError::Diagnostic(diagnostic) = error {
            pg_error.detail.clone_from(&diagnostic.detail);
            pg_error.hint.clone_from(&diagnostic.hint);
        }
        pg_error.table = descriptor.table;
        pg_error.column = descriptor.column;
        pg_error.constraint = descriptor.constraint;
        pg_error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_preserve_native_base64_truncation_hint() {
        // Arrange
        let error = CassieError::InvalidParameterValue("invalid base64 end sequence".into());
        let hint = "Input data is missing padding, is truncated, or is otherwise corrupted.";
        // Act
        let mapped = cassie_pg_error(&error);
        let unrelated = cassie_pg_error(&CassieError::InvalidParameterValue(
            "invalid base64 symbol".into(),
        ));
        // Assert
        assert_eq!(mapped.code, "22023");
        assert_eq!(mapped.hint.as_deref(), Some(hint));
        assert_eq!(unrelated.hint, None);
    }

    #[test]
    fn should_map_retryable_storage_errors_to_cannot_connect_now_sqlstate() {
        // Arrange
        let error = CassieError::StorageRetryable("temporary storage unavailable: fenced".into());

        // Act
        let pg_error = PgWireError::from_cassie_error(PgWireSeverity::Error, &error);

        // Assert
        assert_eq!(pg_error.code, "57P03");
        assert_eq!(pg_error.severity.as_str(), "ERROR");
    }
}
