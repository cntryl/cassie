//! Operators over native `numeric` and `real` operands.
//!
//! PostgreSQL resolves `numeric op integer` to the numeric operator,
//! `numeric op double precision` and `real op integer` to the double
//! precision operator, and `real op real` to the real operator.
use std::cmp::Ordering;

use super::{BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::extended::numeric::{math, Numeric};
use crate::types::extended::TypeInputError;
use crate::types::typed::TypedValue;

fn input_error(error: TypeInputError) -> QueryError {
    QueryError::Cassie(CassieError::TypeInput(error))
}

/// The operand class an operator resolves over.
enum Operands {
    Numeric(Numeric, Numeric),
    Real(f32, f32),
    Double(f64, f64),
}

fn numeric_operand(value: &ScalarValue) -> Option<Result<Numeric, QueryError>> {
    match value {
        ScalarValue::Typed(TypedValue::Numeric(value)) => Some(Ok(value.clone())),
        ScalarValue::Int(value) => Some(Ok(Numeric::from_i64(*value))),
        ScalarValue::Str(text) => {
            Some(crate::types::extended::numeric::parse_text(text).map_err(input_error))
        }
        _ => None,
    }
}

fn is_numeric(value: &ScalarValue) -> bool {
    matches!(value, ScalarValue::Typed(TypedValue::Numeric(_)))
}

fn is_real(value: &ScalarValue) -> bool {
    matches!(value, ScalarValue::Typed(TypedValue::Real(_)))
}

fn real_operand(value: &ScalarValue) -> Option<Result<f32, QueryError>> {
    match value {
        ScalarValue::Typed(TypedValue::Real(value)) => Some(Ok(*value)),
        ScalarValue::Str(text) => {
            Some(crate::types::extended::real::parse_text(text).map_err(input_error))
        }
        _ => None,
    }
}

fn double_operand(value: &ScalarValue, checked_numeric: bool) -> Option<Result<f64, QueryError>> {
    match value {
        ScalarValue::Typed(TypedValue::Numeric(value)) if checked_numeric => {
            Some(super::functions::math::numeric_as_f64(value))
        }
        ScalarValue::Str(text) => text.trim().parse::<f64>().ok().map(Ok),
        other => other.to_f64().map(Ok),
    }
}

/// Classifies operands when either side is a native numeric family value.
fn classify(
    left: &ScalarValue,
    right: &ScalarValue,
    checked_numeric: bool,
) -> Option<Result<Operands, QueryError>> {
    let typed = |value: &ScalarValue| is_numeric(value) || is_real(value);
    if !typed(left) && !typed(right) {
        return None;
    }
    if is_numeric(left) || is_numeric(right) {
        if let (Some(left), Some(right)) = (numeric_operand(left), numeric_operand(right)) {
            return Some(left.and_then(|left| right.map(|right| Operands::Numeric(left, right))));
        }
    }
    if is_real(left) || is_real(right) {
        if let (Some(left), Some(right)) = (real_operand(left), real_operand(right)) {
            return Some(left.and_then(|left| right.map(|right| Operands::Real(left, right))));
        }
    }
    double_operand(left, checked_numeric)
        .zip(double_operand(right, checked_numeric))
        .map(|(left, right)| left.and_then(|left| right.map(|right| Operands::Double(left, right))))
}

/// PostgreSQL's float order: NaN equals NaN and sorts above every number.
fn float_order(left: f64, right: f64) -> Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
    }
}

/// Compares operands when either side is `numeric` or `real`.
pub(super) fn compare(left: &ScalarValue, right: &ScalarValue) -> Option<Ordering> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return None;
    }
    match classify(left, right, false)?.ok()? {
        Operands::Numeric(left, right) => Some(left.compare(&right)),
        Operands::Real(left, right) => Some(float_order(f64::from(left), f64::from(right))),
        Operands::Double(left, right) => Some(float_order(left, right)),
    }
}

fn real_result(value: f32, finite_inputs: bool) -> Result<ScalarValue, QueryError> {
    if finite_inputs && value.is_infinite() {
        return Err(input_error(TypeInputError {
            sqlstate: "22003",
            message: "value out of range: overflow".to_string(),
        }));
    }
    Ok(ScalarValue::Typed(TypedValue::Real(value)))
}

fn double_result(value: f64, finite_inputs: bool) -> Result<ScalarValue, QueryError> {
    if finite_inputs && value.is_infinite() {
        return Err(QueryError::Cassie(CassieError::FloatInput(
            crate::types::FloatInputError::Range,
        )));
    }
    Ok(ScalarValue::Float(value))
}

fn division_by_zero() -> QueryError {
    QueryError::General("division by zero".to_string())
}

/// Evaluates arithmetic when either operand is `numeric` or `real`.
pub(super) fn arithmetic(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Option<Result<ScalarValue, QueryError>> {
    arithmetic_with_resources(left, op, right, None, None)
}

pub(super) fn arithmetic_with_resources(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
    memory: Option<&super::EvaluationMemory>,
) -> Option<Result<ScalarValue, QueryError>> {
    if !matches!(
        op,
        BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Modulo
            | BinaryOp::Power
    ) {
        return None;
    }
    let typed = |value: &ScalarValue| is_numeric(value) || is_real(value);
    if !typed(left) && !typed(right) {
        return None;
    }
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Some(Ok(ScalarValue::Null));
    }
    let operands = match classify(left, right, matches!(op, BinaryOp::Power))? {
        Ok(operands) => operands,
        Err(error) => return Some(Err(error)),
    };
    Some(match operands {
        Operands::Numeric(left, right) => numeric_arithmetic(&left, op, &right, controls, memory),
        Operands::Real(left, right) => {
            let finite = left.is_finite() && right.is_finite();
            match op {
                BinaryOp::Add => real_result(left + right, finite),
                BinaryOp::Sub => real_result(left - right, finite),
                BinaryOp::Mul => real_result(left * right, finite),
                BinaryOp::Div if right == 0.0 => Err(division_by_zero()),
                BinaryOp::Div => real_result(left / right, finite),
                _ => double_arithmetic(f64::from(left), op, f64::from(right)),
            }
        }
        Operands::Double(left, right) => double_arithmetic(left, op, right),
    })
}

fn double_arithmetic(left: f64, op: &BinaryOp, right: f64) -> Result<ScalarValue, QueryError> {
    let finite = left.is_finite() && right.is_finite();
    match op {
        BinaryOp::Add => double_result(left + right, finite),
        BinaryOp::Sub => double_result(left - right, finite),
        BinaryOp::Mul => double_result(left * right, finite),
        BinaryOp::Div if right == 0.0 => Err(division_by_zero()),
        BinaryOp::Div => double_result(left / right, finite),
        BinaryOp::Power => super::functions::math::power(left, right).map(ScalarValue::Float),
        _ => Err(QueryError::General(
            "operator does not exist: double precision % double precision".to_string(),
        )),
    }
}

fn numeric_arithmetic(
    left: &Numeric,
    op: &BinaryOp,
    right: &Numeric,
    controls: Option<&crate::runtime::QueryExecutionControls>,
    memory: Option<&super::EvaluationMemory>,
) -> Result<ScalarValue, QueryError> {
    if *op == BinaryOp::Power {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let preflight_bytes =
            math::power_preflight_workspace_bytes(left, right).ok_or_else(|| {
                QueryError::Cassie(CassieError::ResourceLimit(
                    "numeric power preflight workspace size overflow".into(),
                ))
            })?;
        let mut owner = memory
            .map(|memory| memory.scope(preflight_bytes))
            .transpose()?;
        let mut temporary = if owner.is_none() {
            controls
                .map(|controls| controls.reserve_query_memory(preflight_bytes))
                .transpose()?
        } else {
            None
        };
        let bytes = math::power_workspace_bytes(left, right).ok_or_else(|| {
            QueryError::Cassie(CassieError::ResourceLimit(
                "numeric power workspace size overflow".into(),
            ))
        })?;
        if let Some(owner) = owner.as_mut() {
            owner.ensure_admitted(bytes)?;
        } else if let Some(temporary) = temporary.as_mut() {
            if bytes > preflight_bytes {
                temporary.try_grow(bytes - preflight_bytes)?;
            }
        }
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let mut check = || {
            controls.map_or(
                Ok(()),
                crate::runtime::QueryExecutionControls::check_interruption,
            )
        };
        let result =
            math::power_with_check(left, right, &mut check).map_err(|error| match error {
                math::PowerError::TypeInput(error) => input_error(error),
                math::PowerError::Interruption(error) => error.into(),
            })?;
        let result = TypedValue::Numeric(result);
        if let Some(owner) = owner.as_mut() {
            owner.retain(result.heap_bytes());
        }
        return Ok(ScalarValue::Typed(result));
    }
    let result = match op {
        BinaryOp::Add => math::add(left, right),
        BinaryOp::Sub => math::subtract(left, right),
        BinaryOp::Mul => math::multiply(left, right),
        BinaryOp::Div => math::divide(left, right),
        BinaryOp::Modulo => math::modulo(left, right),
        _ => unreachable!("numeric arithmetic dispatch receives supported operations"),
    };
    result
        .map(|value| ScalarValue::Typed(TypedValue::Numeric(value)))
        .map_err(|error| {
            if error.sqlstate == "22012" {
                division_by_zero()
            } else {
                input_error(error)
            }
        })
}

/// Unary minus, plus and `@` over `numeric` and `real`.
pub(super) fn unary(
    value: &ScalarValue,
    op: crate::sql::ast::UnaryNumericOp,
) -> Option<ScalarValue> {
    use crate::sql::ast::UnaryNumericOp;
    match (value, op) {
        (ScalarValue::Typed(TypedValue::Numeric(value)), UnaryNumericOp::Minus) => {
            Some(ScalarValue::Typed(TypedValue::Numeric(math::negate(value))))
        }
        (ScalarValue::Typed(TypedValue::Numeric(value)), UnaryNumericOp::Absolute) => {
            Some(ScalarValue::Typed(TypedValue::Numeric(math::abs(value))))
        }
        (ScalarValue::Typed(TypedValue::Real(value)), UnaryNumericOp::Minus) => {
            Some(ScalarValue::Typed(TypedValue::Real(-*value)))
        }
        (ScalarValue::Typed(TypedValue::Real(value)), UnaryNumericOp::Absolute) => {
            Some(ScalarValue::Typed(TypedValue::Real(value.abs())))
        }
        (
            ScalarValue::Typed(TypedValue::Numeric(_) | TypedValue::Real(_)),
            UnaryNumericOp::Plus,
        ) => Some(value.clone()),
        _ => None,
    }
}
