//! Scoped temporal overload diagnostics on the legacy binder path.

use super::super::coalesce_results::ResultTypes;
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::FunctionCall;
use crate::sql::ParameterSemanticDiagnostic;

pub(super) fn validate(function: &FunctionCall, types: &ResultTypes) -> Result<(), CassieError> {
    if types.declared_function(&function.name).is_some() {
        return Ok(());
    }
    let Some(qualified) = crate::sql::timetz_format_signature::candidate(function, |_| false)
    else {
        return Ok(());
    };
    let diagnostic = ParameterSemanticDiagnostic::ToCharTimeTz { qualified };
    let error = DiagnosticError::new(diagnostic.code(), diagnostic.message())
        .with_hint(diagnostic.hint().unwrap_or_default());
    Err(error.into())
}
