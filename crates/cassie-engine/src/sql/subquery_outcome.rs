//! Runtime-only outcomes attached to the syntax AST by the engine.

use std::sync::{Arc, Mutex};

use crate::runtime::QueryMemoryReservation;
use crate::types::Value;

/// The rows an executed subquery produced, or its deferred execution error.
#[derive(Debug, Clone)]
pub enum SubqueryOutcome {
    Rows(Arc<SubqueryRows>),
    Failed(Arc<Mutex<Option<crate::app::CassieError>>>),
}

/// Wraps a failure so it is raised only if the expression is evaluated.
impl SubqueryOutcome {
    #[must_use]
    pub fn failed(error: crate::app::CassieError) -> Self {
        Self::Failed(Arc::new(Mutex::new(Some(error))))
    }
}

/// Query rows retained under the query's admitted memory reservation.
#[derive(Debug)]
pub struct SubqueryRows {
    pub rows: Vec<Vec<Value>>,
    pub memory: Option<QueryMemoryReservation>,
}

impl SubqueryExprWidthError for crate::sql::ast::SubqueryExpr {
    fn width_error(&self, actual: usize) -> crate::app::CassieError {
        let message = match &self.kind {
            crate::sql::ast::SubqueryKind::Scalar | crate::sql::ast::SubqueryKind::Array => {
                "subquery must return only one column"
            }
            crate::sql::ast::SubqueryKind::Row { left, .. }
            | crate::sql::ast::SubqueryKind::Quantified { left, .. } => {
                if actual > left.len() {
                    "subquery has too many columns"
                } else {
                    "subquery has too few columns"
                }
            }
        };
        crate::app::CassieError::Diagnostic(Box::new(crate::app::DiagnosticError::new(
            "42601", message,
        )))
    }
}

pub trait SubqueryExprWidthError {
    fn width_error(&self, actual: usize) -> crate::app::CassieError;
}
