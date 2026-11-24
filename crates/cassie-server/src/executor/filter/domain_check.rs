//! Domain constraint evaluation and casts into domain types (#828).

use std::collections::HashMap;

use super::{cast_scalar, eval_scalar, ScalarValue};
use crate::app::{CassieError, DiagnosticError};
use crate::executor::batch::BatchRow;
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;
use crate::types::domain::DOMAIN_VALUE_COLUMN;
use crate::types::{DomainCheck, DomainType, Value};

/// How a value failed its domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DomainViolation {
    /// The domain is NOT NULL and the value is SQL NULL.
    NotNull,
    /// The named CHECK constraint evaluated to false.
    Check(String),
}

impl DomainViolation {
    /// The PostgreSQL diagnostic for this violation of `domain`.
    pub(crate) fn into_error(self, domain: &DomainType) -> DiagnosticError {
        match self {
            Self::NotNull => DiagnosticError::new(
                "23502",
                format!("domain {} does not allow null values", domain.name),
            ),
            Self::Check(constraint) => DiagnosticError::new(
                "23514",
                format!(
                    "value for domain {} violates check constraint \"{constraint}\"",
                    domain.name
                ),
            ),
        }
    }
}

fn general(message: String) -> QueryError {
    QueryError::Cassie(CassieError::from(DiagnosticError::new("XX000", message)))
}

/// Evaluates one CHECK constraint against `value`. A CHECK passes when its
/// expression is true or NULL, as in PostgreSQL.
///
/// # Errors
/// Returns an error when the stored expression no longer parses, is not
/// boolean, or fails to evaluate.
pub(crate) fn evaluate_check(check: &DomainCheck, value: &Value) -> Result<bool, QueryError> {
    let expression = crate::sql::parser::parse_expression(&check.sql).map_err(|error| {
        general(format!(
            "domain constraint \"{}\" has an invalid expression: {error}",
            check.name
        ))
    })?;
    let row = BatchRow::new(vec![(DOMAIN_VALUE_COLUMN.to_owned(), value.clone())]);
    let result = eval_scalar(&row, &expression, &[], None, &HashMap::new(), None, None)?;
    match result {
        ScalarValue::Bool(passes) => Ok(passes),
        ScalarValue::Null => Ok(true),
        _ => Err(QueryError::Cassie(CassieError::from(DiagnosticError::new(
            "42804",
            format!(
                "argument of domain constraint \"{}\" must be type boolean",
                check.name
            ),
        )))),
    }
}

/// The first constraint of `domain` that `value` violates: NOT NULL first, then
/// the CHECK constraints in name order, as PostgreSQL evaluates them.
///
/// # Errors
/// Returns an error when a constraint expression cannot be evaluated.
pub(crate) fn first_violation(
    domain: &DomainType,
    value: &Value,
) -> Result<Option<DomainViolation>, QueryError> {
    if domain.not_null && matches!(value, Value::Null) {
        return Ok(Some(DomainViolation::NotNull));
    }
    let mut checks = domain.checks.iter().collect::<Vec<_>>();
    checks.sort_by(|left, right| left.name.cmp(&right.name));
    for check in checks {
        if !evaluate_check(check, value)? {
            return Ok(Some(DomainViolation::Check(check.name.clone())));
        }
    }
    Ok(None)
}

fn enforce(domain: &DomainType, value: &Value) -> Result<(), QueryError> {
    match first_violation(domain, value)? {
        Some(violation) => Err(QueryError::Cassie(CassieError::from(
            violation.into_error(domain),
        ))),
        None => Ok(()),
    }
}

/// `value::domain`: casts to the base type, then enforces the domain.
pub(super) fn cast(
    value: &ScalarValue,
    domain: &DomainType,
    controls: Option<&QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    let cast = cast_scalar(value, &domain.base, controls)?;
    enforce(domain, &cast.to_value())?;
    Ok(cast)
}

/// `NULL::domain`: NOT NULL and CHECK constraints apply to NULL as well.
pub(super) fn cast_null(domain: &DomainType) -> Result<ScalarValue, QueryError> {
    enforce(domain, &Value::Null)?;
    Ok(ScalarValue::Null)
}
