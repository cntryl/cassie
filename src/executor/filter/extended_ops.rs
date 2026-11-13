//! Binary operators over network, bit-string and `pg_lsn` operands.
use super::{BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::extended::{operations, TypeInputError, TypedOperator};
use crate::types::typed::TypedValue;
use crate::types::EnumValue;

fn error(error: TypeInputError) -> QueryError {
    QueryError::Cassie(CassieError::TypeInput(error))
}

const fn owned(value: &ScalarValue) -> bool {
    matches!(
        value,
        ScalarValue::Typed(typed) if typed.as_record().is_some() || operations::handles_value(typed)
    )
}

/// Compares an owned typed value with another operand: another value of its
/// comparison class, or text input for its type.
pub(super) fn compare(left: &ScalarValue, right: &ScalarValue) -> Option<std::cmp::Ordering> {
    if !owned(left) && !owned(right) {
        return None;
    }
    match (left, right) {
        (
            ScalarValue::Typed(TypedValue::Enum(left)),
            ScalarValue::Typed(TypedValue::Enum(right)),
        ) => {
            // Two different enum types have no ordering.
            (left.descriptor().oid == right.descriptor().oid)
                .then(|| left.order_key().cmp(&right.order_key()))
        }
        (ScalarValue::Typed(left), ScalarValue::Typed(right)) => operations::compare(left, right),
        (ScalarValue::Typed(TypedValue::Enum(value)), ScalarValue::Str(text)) => {
            // An untyped label meets the enum's input function.
            EnumValue::parse_label(value.descriptor(), text)
                .ok()
                .map(|other| value.order_key().cmp(&other.order_key()))
        }
        (ScalarValue::Typed(typed), ScalarValue::Str(text)) => typed
            .kind()
            .and_then(|kind| kind.parse_text(text).ok())
            .and_then(|other| operations::compare(typed, &other)),
        (ScalarValue::Str(_), ScalarValue::Typed(_)) => {
            compare(right, left).map(std::cmp::Ordering::reverse)
        }
        _ => None,
    }
}

/// Evaluates `left op right` when either operand belongs to an owned family;
/// `None` leaves the operator to the scalar kernels.
pub(super) fn binary(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Option<Result<ScalarValue, QueryError>> {
    if matches!(op, BinaryOp::Quantified { .. }) {
        // `x op ANY|ALL (array)` unpacks the array first; each element then
        // meets this module through the element comparison.
        return None;
    }
    let null = matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null);
    if !owned(left) && !owned(right) {
        return matches!(op, BinaryOp::Typed { .. }).then(|| {
            if null {
                Ok(ScalarValue::Null)
            } else {
                Err(undefined(left, op, right))
            }
        });
    }
    if let Some(result) = comparison(left, op, right, null) {
        return Some(result);
    }
    if null {
        return Some(Ok(ScalarValue::Null));
    }
    let Some(operator) = op.typed_operator() else {
        return Some(Err(undefined(left, op, right)));
    };
    let result = operations::operator(operator, &left.to_value(), &right.to_value());
    if result.is_none() && operator == TypedOperator::Concat {
        // Text concatenation renders the family's output text.
        return None;
    }
    Some(
        result
            .unwrap_or_else(|| Err(undefined_error(left, op, right)))
            .map(|value| super::scalar_from_value(&value))
            .map_err(error),
    )
}

fn comparison(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    null: bool,
) -> Option<Result<ScalarValue, QueryError>> {
    if !null {
        if let Some(Err(error)) = enum_label_input(left, right) {
            return Some(Err(error));
        }
    }
    if !null && record_against_text(left, right) {
        return Some(Err(QueryError::Cassie(CassieError::from(
            crate::app::DiagnosticError::new(
                "0A000",
                "input of anonymous composite types is not implemented",
            ),
        ))));
    }
    let ordering = || compare(left, right).ok_or_else(|| undefined(left, op, right));
    let ordered = |test: fn(std::cmp::Ordering) -> bool| {
        Some(if null {
            Ok(ScalarValue::Null)
        } else {
            ordering().map(|ordering| ScalarValue::Bool(test(ordering)))
        })
    };
    match op {
        BinaryOp::Eq => ordered(std::cmp::Ordering::is_eq),
        BinaryOp::NotEq => ordered(std::cmp::Ordering::is_ne),
        BinaryOp::Lt => ordered(std::cmp::Ordering::is_lt),
        BinaryOp::Lte => ordered(std::cmp::Ordering::is_le),
        BinaryOp::Gt => ordered(std::cmp::Ordering::is_gt),
        BinaryOp::Gte => ordered(std::cmp::Ordering::is_ge),
        BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom => {
            let equal = match (left, right) {
                (ScalarValue::Null, ScalarValue::Null) => Ok(true),
                (ScalarValue::Null, _) | (_, ScalarValue::Null) => Ok(false),
                _ => ordering().map(std::cmp::Ordering::is_eq),
            };
            Some(
                equal.map(|equal| {
                    ScalarValue::Bool(equal == matches!(op, BinaryOp::IsNotDistinctFrom))
                }),
            )
        }
        _ => None,
    }
}

/// An enum compared with an untyped label: the label must be valid input for
/// the enum (`22P02`/`55P04`), as PostgreSQL resolves it with `enum_in`.
fn enum_label_input(left: &ScalarValue, right: &ScalarValue) -> Option<Result<(), QueryError>> {
    let ((ScalarValue::Typed(TypedValue::Enum(value)), ScalarValue::Str(label))
    | (ScalarValue::Str(label), ScalarValue::Typed(TypedValue::Enum(value)))) = (left, right)
    else {
        return None;
    };
    Some(
        EnumValue::parse_label(value.descriptor(), label)
            .map(|_| ())
            .map_err(|error| {
                QueryError::Cassie(CassieError::from(super::enum_cast::input_diagnostic(
                    &error,
                )))
            }),
    )
}

/// A record compared with an untyped string literal, which PostgreSQL cannot
/// coerce to a record type.
fn record_against_text(left: &ScalarValue, right: &ScalarValue) -> bool {
    let is_record = |value: &ScalarValue| matches!(value, ScalarValue::Typed(typed) if typed.as_record().is_some());
    (is_record(left) && matches!(right, ScalarValue::Str(_)))
        || (matches!(left, ScalarValue::Str(_)) && is_record(right))
}

fn type_label(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Typed(TypedValue::Enum(value)) => value.descriptor().name.clone(),
        ScalarValue::Typed(typed) => typed.as_record().map_or_else(
            || {
                typed
                    .kind()
                    .map_or("record", crate::types::extended::ExtendedType::name)
                    .to_string()
            },
            |record| record.descriptor().name.clone(),
        ),
        ScalarValue::Int(_) => "integer".to_string(),
        ScalarValue::Float(_) => "double precision".to_string(),
        ScalarValue::Bool(_) => "boolean".to_string(),
        ScalarValue::Str(_) | ScalarValue::Json(_) => "text".to_string(),
        ScalarValue::Array(_) => "array".to_string(),
        ScalarValue::Null => "unknown".to_string(),
    }
}

fn symbol(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Eq => "=",
        BinaryOp::NotEq => "<>",
        BinaryOp::Lt => "<",
        BinaryOp::Lte => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Gte => ">=",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Modulo => "%",
        BinaryOp::Power => "^",
        _ => match op.typed_operator() {
            Some(TypedOperator::Add) => "+",
            Some(TypedOperator::Sub) => "-",
            Some(TypedOperator::And) => "&",
            Some(TypedOperator::Or) => "|",
            Some(TypedOperator::Xor) => "#",
            Some(TypedOperator::ShiftLeft) => "<<",
            Some(TypedOperator::ShiftRight) => ">>",
            Some(TypedOperator::Concat) => "||",
            Some(TypedOperator::ContainedOrEqual) => "<<=",
            Some(TypedOperator::ContainsOrEqual) => ">>=",
            Some(TypedOperator::Overlap) => "&&",
            None => "?",
        },
    }
}

fn undefined_error(left: &ScalarValue, op: &BinaryOp, right: &ScalarValue) -> TypeInputError {
    TypeInputError::new(
        "42883",
        format!(
            "operator does not exist: {} {} {}",
            type_label(left),
            symbol(op),
            type_label(right)
        ),
    )
}

fn undefined(left: &ScalarValue, op: &BinaryOp, right: &ScalarValue) -> QueryError {
    error(undefined_error(left, op, right))
}
