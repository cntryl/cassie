//! PostgreSQL diagnostics that carry `DETAIL` and `HINT` fields.

/// An error whose SQLSTATE, message, detail and hint are reported as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticError {
    pub sql_state: &'static str,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

impl DiagnosticError {
    #[must_use]
    pub fn new(sql_state: &'static str, message: impl Into<String>) -> Self {
        Self {
            sql_state,
            message: message.into(),
            detail: None,
            hint: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl From<DiagnosticError> for super::CassieError {
    fn from(error: DiagnosticError) -> Self {
        Self::Diagnostic(Box::new(error))
    }
}
