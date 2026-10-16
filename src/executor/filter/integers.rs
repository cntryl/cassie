//! Checked conversions and resolved operators for existing integer domains.
use super::{QueryError, ScalarValue};
use crate::types::integer::{self, IntegerArithmeticError, IntegerInputError};
use crate::types::{IntegerOperation, IntegerWidth};

pub(super) fn cast(value: &ScalarValue, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    let value = match value {
        ScalarValue::Int(value) => *value,
        ScalarValue::Bool(value) => i64::from(*value),
        ScalarValue::Float(value) if value.is_finite() => format!("{:.0}", value.round_ties_even())
            .parse::<i64>()
            .map_err(|_| range(width))?,
        ScalarValue::Str(input) => integer::parse(input, width).map_err(|error| match error {
            IntegerInputError::Syntax => QueryError::General(width.syntax_message().into()),
            IntegerInputError::Range => range(width),
        })?,
        // Preserve Cassie's existing decimal-only JSON scalar conversion.
        // PostgreSQL text integer grammar must not admit new JSON spellings.
        ScalarValue::Json(input) => input.parse::<i64>().map_err(|error| {
            if matches!(
                error.kind(),
                std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
            ) {
                range(width)
            } else {
                QueryError::General(width.syntax_message().into())
            }
        })?,
        ScalarValue::Null => return Ok(ScalarValue::Null),
        ScalarValue::Float(_) => return Err(range(width)),
    };
    if !width.contains(value) {
        return Err(range(width));
    }
    Ok(ScalarValue::Int(value))
}

pub(super) fn arithmetic(
    left: &ScalarValue,
    operation: IntegerOperation,
    width: IntegerWidth,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Int(left), ScalarValue::Int(right)) = (left, right) else {
        return Err(QueryError::General(
            "integer operator requires integer operands".into(),
        ));
    };
    integer::arithmetic(width, operation, *left, *right)
        .map(ScalarValue::Int)
        .map_err(|error| match error {
            IntegerArithmeticError::Range => range(width),
            IntegerArithmeticError::DivisionByZero => {
                QueryError::General("division by zero".into())
            }
        })
}

fn range(width: IntegerWidth) -> QueryError {
    QueryError::General(width.range_message().into())
}

pub(super) fn negate(value: &ScalarValue, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Null => Ok(ScalarValue::Null),
        ScalarValue::Int(value) => integer::negate(width, *value)
            .map(ScalarValue::Int)
            .map_err(|_| range(width)),
        _ => Err(QueryError::General(
            "integer negation requires an integer operand".into(),
        )),
    }
}

pub(super) fn input(input: &str, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    integer::parse(input, width)
        .map(ScalarValue::Int)
        .map_err(|error| match error {
            IntegerInputError::Syntax => QueryError::General(width.syntax_message().into()),
            IntegerInputError::Range => range(width),
        })
}
