//! Date/time operators, comparisons and casts in the scalar evaluator.
use std::cmp::Ordering;
use std::sync::Arc;

use super::{BinaryOp, EvalContext, QueryError, ScalarValue};
use crate::types::extended::temporal::ops::{self, ArithmeticOp, Number, Operand, Outcome};
use crate::types::extended::temporal::TemporalSession;
use crate::types::extended::{ExtendedType, TypeInputError};
use crate::types::typed::TypedValue;
use crate::types::DataType;

/// A borrowed or statement-owned temporal session.
pub(crate) enum SessionRef {
    Statement(Arc<TemporalSession>),
    Default,
}

impl std::ops::Deref for SessionRef {
    type Target = TemporalSession;

    fn deref(&self) -> &TemporalSession {
        match self {
            Self::Statement(session) => session,
            Self::Default => crate::types::extended::temporal::default_session(),
        }
    }
}

/// The temporal session of the executing statement.
pub(crate) fn session_of(controls: Option<&crate::runtime::QueryExecutionControls>) -> SessionRef {
    controls
        .and_then(crate::runtime::QueryExecutionControls::temporal_session)
        .map_or(SessionRef::Default, SessionRef::Statement)
}

pub(super) fn context_controls(
    context: EvalContext<'_>,
) -> Option<&crate::runtime::QueryExecutionControls> {
    context.controls.or_else(|| {
        context
            .session
            .and_then(crate::app::CassieSession::statement_execution_controls)
    })
}

pub(crate) fn type_error(error: TypeInputError) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::TypeInput(error))
}

fn temporal(value: &ScalarValue) -> Option<&TypedValue> {
    match value {
        ScalarValue::Typed(typed) if ops::is_temporal(typed) => Some(typed),
        _ => None,
    }
}

fn parse_as(
    text: &str,
    kind: ExtendedType,
    session: &TemporalSession,
) -> Result<TypedValue, QueryError> {
    kind.parse_text_in(text, session).map_err(type_error)
}

fn operand(value: &ScalarValue) -> Option<Operand> {
    match value {
        ScalarValue::Typed(typed) if !ops::is_temporal(typed) => typed
            .as_f64()
            .map(|value| Operand::Number(Number::Float(value))),
        ScalarValue::Typed(typed) => Some(Operand::Typed(typed.clone())),
        ScalarValue::Int(value) => Some(Operand::Number(Number::Int(*value))),
        ScalarValue::Float(value) => Some(Operand::Number(Number::Float(*value))),
        _ => None,
    }
}

const INTERVAL: ExtendedType = ExtendedType::Interval { typmod: -1 };

/// Resolves an untyped string operand against its temporal partner.
fn resolve_unknown(
    value: &ScalarValue,
    partner: &TypedValue,
    op: &BinaryOp,
    session: &TemporalSession,
) -> Result<Option<Operand>, QueryError> {
    let ScalarValue::Str(text) = value else {
        return Ok(operand(value));
    };
    let kind = match op {
        BinaryOp::Add | BinaryOp::Sub if !matches!(partner, TypedValue::Interval(_)) => {
            if let Ok(interval) = parse_as(text, INTERVAL, session) {
                return Ok(Some(Operand::Typed(interval)));
            }
            partner.kind()
        }
        _ => partner.kind(),
    };
    parse_as(text, kind, session).map(|typed| Some(Operand::Typed(typed)))
}

/// Evaluates a binary operator with a temporal operand; `None` otherwise.
pub(super) fn binary(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    context: EvalContext<'_>,
) -> Option<Result<ScalarValue, QueryError>> {
    binary_with_controls(left, op, right, context_controls(context))
}

/// [`super::binary_scalar`] with the temporal overloads applied first.
pub(super) fn binary_or_scalar(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    binary_with_controls(left, op, right, controls)
        .unwrap_or_else(|| super::binary_scalar(left, op, right))
}

fn binary_with_controls(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<ScalarValue, QueryError>> {
    let partner = temporal(left).or_else(|| temporal(right))?.clone();
    let session = &*session_of(controls);
    match op {
        BinaryOp::Concat => return Some(Ok(concatenate(left, right, session))),
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::IsDistinctFrom
        | BinaryOp::IsNotDistinctFrom => {}
        _ => return None,
    }
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return match op {
            BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::And
            | BinaryOp::Or => None,
            _ => Some(Ok(ScalarValue::Null)),
        };
    }
    let resolve = |value: &ScalarValue| resolve_unknown(value, &partner, op, session);
    let evaluated = (|| -> Result<Option<ScalarValue>, QueryError> {
        let (Some(left), Some(right)) = (resolve(left)?, resolve(right)?) else {
            return Ok(None);
        };
        let arithmetic = match op {
            BinaryOp::Add => ArithmeticOp::Add,
            BinaryOp::Sub => ArithmeticOp::Sub,
            BinaryOp::Mul => ArithmeticOp::Mul,
            BinaryOp::Div => ArithmeticOp::Div,
            _ => return Ok(comparison(&left, op, &right, session)),
        };
        match ops::arithmetic(&left, arithmetic, &right, session) {
            Some(Ok(Outcome::Typed(value))) => Ok(Some(ScalarValue::Typed(value))),
            Some(Ok(Outcome::Int(value))) => Ok(Some(ScalarValue::Int(value))),
            Some(Err(error)) => Err(type_error(error)),
            None => Err(QueryError::Cassie(crate::app::CassieError::Execution(
                format!(
                    "operator does not exist: {} {} {}",
                    operand_type(&left),
                    operator_symbol(op),
                    operand_type(&right)
                ),
            ))),
        }
    })();
    evaluated.transpose()
}

/// `||` renders a temporal operand in the session's styles.
fn concatenate(left: &ScalarValue, right: &ScalarValue, session: &TemporalSession) -> ScalarValue {
    let text = |value: &ScalarValue| match value {
        ScalarValue::Typed(typed) => Some(typed.text_in(session)),
        other => super::cast_to_text(other),
    };
    match (text(left), text(right)) {
        (Some(mut left), Some(right)) => {
            left.push_str(&right);
            ScalarValue::Str(left)
        }
        _ => ScalarValue::Null,
    }
}

fn operand_type(value: &Operand) -> &'static str {
    match value {
        Operand::Typed(typed) => crate::types::extended::temporal_display_name(typed.kind()),
        Operand::Number(Number::Int(_)) => "integer",
        Operand::Number(Number::Float(_)) => "double precision",
    }
}

const fn operator_symbol(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        _ => "?",
    }
}

fn comparison(
    left: &Operand,
    op: &BinaryOp,
    right: &Operand,
    session: &TemporalSession,
) -> Option<ScalarValue> {
    let (Operand::Typed(left), Operand::Typed(right)) = (left, right) else {
        return None;
    };
    let ordering = ops::compare(left, right, session)?;
    let result = match op {
        BinaryOp::Eq | BinaryOp::IsNotDistinctFrom => ordering == Ordering::Equal,
        BinaryOp::NotEq | BinaryOp::IsDistinctFrom => ordering != Ordering::Equal,
        BinaryOp::Lt => ordering == Ordering::Less,
        BinaryOp::Lte => ordering != Ordering::Greater,
        BinaryOp::Gt => ordering == Ordering::Greater,
        BinaryOp::Gte => ordering != Ordering::Less,
        _ => return None,
    };
    Some(ScalarValue::Bool(result))
}

/// Orders two scalars when either is temporal (for `IN`, `BETWEEN`, `CASE`).
pub(super) fn compare_scalars(left: &ScalarValue, right: &ScalarValue) -> Option<Ordering> {
    let session = crate::types::extended::temporal::default_session();
    let partner = temporal(left).or_else(|| temporal(right))?;
    let resolve = |value: &ScalarValue| match value {
        ScalarValue::Typed(typed) => Some(typed.clone()),
        ScalarValue::Str(text) => partner.kind().parse_text_in(text, session).ok(),
        _ => None,
    };
    ops::compare(&resolve(left)?, &resolve(right)?, session)
}

/// Unary minus on an interval.
pub(super) fn negate(value: &ScalarValue) -> Option<Result<ScalarValue, QueryError>> {
    let ScalarValue::Typed(TypedValue::Interval(interval)) = value else {
        return None;
    };
    Some(
        interval
            .negate()
            .map(|value| ScalarValue::Typed(TypedValue::Interval(value)))
            .map_err(|_| {
                type_error(crate::types::extended::temporal::range_error(
                    "interval out of range",
                ))
            }),
    )
}

/// The native target of a temporal cast.
const fn temporal_target(data_type: &DataType) -> Option<ExtendedType> {
    match data_type {
        DataType::Date => Some(ExtendedType::Date),
        DataType::Time => Some(ExtendedType::Time { precision: None }),
        DataType::Timestamp => Some(ExtendedType::Timestamp { precision: None }),
        DataType::Extended(
            kind @ (ExtendedType::Date
            | ExtendedType::Time { .. }
            | ExtendedType::TimeTz { .. }
            | ExtendedType::Timestamp { .. }
            | ExtendedType::TimestampTz { .. }
            | ExtendedType::Interval { .. }),
        ) => Some(*kind),
        _ => None,
    }
}

/// Casts to or from a temporal type under the statement session.
pub(super) fn cast(
    value: &ScalarValue,
    data_type: &DataType,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<ScalarValue, QueryError>> {
    if temporal_target(data_type).is_none() && temporal(value).is_none() {
        return None;
    }
    let session = &*session_of(controls);
    if let Some(kind) = temporal_target(data_type) {
        let source = match value {
            ScalarValue::Json(text) => {
                crate::types::Value::String(text.trim_matches('"').to_string())
            }
            other => other.to_value(),
        };
        if matches!(&source, crate::types::Value::Typed(typed) if !ops::is_temporal(typed)) {
            return None;
        }
        return Some(
            kind.cast_from_in(&source, session)
                .map(ScalarValue::Typed)
                .map_err(type_error),
        );
    }
    let typed = temporal(value)?;
    match data_type {
        DataType::Text | DataType::Varchar { length: None } => {
            Some(Ok(ScalarValue::Str(typed.text_in(session))))
        }
        _ => None,
    }
}
