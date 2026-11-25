//! Query-controlled value parsing that depends on Cassie runtime ownership.

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::temporal::{self, TemporalSession};
use crate::types::extended::{ExtendedType, TypeInputError};
use crate::types::{TypedValue, Value};

#[path = "controlled_temporal.rs"]
pub(crate) mod controlled_temporal;

/// Parses a typed value while charging temporal scratch to the query.
///
/// # Errors
/// Returns a type-input error or a query control/resource error.
pub(crate) fn parse_text_with_controls(
    kind: ExtendedType,
    text: &str,
    session: &TemporalSession,
    controls: &QueryExecutionControls,
) -> Result<TypedValue, CassieError> {
    if is_temporal(kind) {
        controlled_temporal::parse_text(kind, text, session, controls)
    } else {
        kind.parse_text_in(text, session).map_err(CassieError::from)
    }
}

/// Casts a value while preserving query-controlled temporal parsing.
///
/// # Errors
/// Returns a type-input error or a query control/resource error.
pub(crate) fn cast_from_with_controls(
    kind: ExtendedType,
    value: &Value,
    session: &TemporalSession,
    controls: &QueryExecutionControls,
) -> Result<TypedValue, CassieError> {
    if is_temporal(kind) {
        if let Value::String(text) = value {
            return parse_text_with_controls(kind, text, session, controls);
        }
    }
    kind.cast_from_in(value, session).map_err(CassieError::from)
}

/// Decodes a temporal bind parameter while charging query scratch memory.
///
/// # Errors
/// Returns a type-input result or a query control/resource error.
pub fn parameter_from_text_with_controls(
    kind: ExtendedType,
    text: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<Result<TypedValue, TypeInputError>>, CassieError> {
    if !is_temporal(kind) {
        return Ok(None);
    }
    let utc = temporal::default_session();
    let parsed = parse_controlled(kind, text, utc, controls)?;
    if matches!(
        kind,
        ExtendedType::TimestampTz { .. } | ExtendedType::TimeTz { .. }
    ) {
        let shifted = TemporalSession {
            zone: temporal::zone::fixed_zone(3600),
            ..utc.clone()
        };
        let shifted_result = parse_controlled(kind, text, &shifted, controls)?;
        if parsed.as_ref().ok() != shifted_result.as_ref().ok() {
            return Ok(None);
        }
    }
    Ok(Some(parsed))
}

fn parse_controlled(
    kind: ExtendedType,
    text: &str,
    session: &TemporalSession,
    controls: &QueryExecutionControls,
) -> Result<Result<TypedValue, TypeInputError>, CassieError> {
    match controlled_temporal::parse_text(kind, text, session, controls) {
        Ok(value) => Ok(Ok(value)),
        Err(CassieError::TypeInput(error)) => Ok(Err(error)),
        Err(error) => Err(error),
    }
}

const fn is_temporal(kind: ExtendedType) -> bool {
    matches!(
        kind,
        ExtendedType::Date
            | ExtendedType::Time { .. }
            | ExtendedType::TimeTz { .. }
            | ExtendedType::Timestamp { .. }
            | ExtendedType::TimestampTz { .. }
            | ExtendedType::Interval { .. }
    )
}
