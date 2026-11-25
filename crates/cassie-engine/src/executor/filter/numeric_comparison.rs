//! Temporary comparison carriers; stored and projected values remain unchanged.

use std::borrow::Cow;

use super::{cast_scalar, scalar_from_value, DataType, QueryError, ScalarValue, Value};
use crate::sql::numeric_comparison::NumericComparisonDomain;

pub(crate) fn comparison_key_value(
    value: &Value,
    domain: NumericComparisonDomain,
) -> Result<Cow<'_, Value>, QueryError> {
    if domain == NumericComparisonDomain::Exact || matches!(value, Value::Float64(_) | Value::Null)
    {
        return Ok(Cow::Borrowed(value));
    }
    let normalized = float_scalar(&scalar_from_value(value))?;
    Ok(Cow::Owned(normalized.to_value()))
}

pub(super) fn float_scalar(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Float(_) | ScalarValue::Null => Ok(value.clone()),
        ScalarValue::Int(value) => Ok(ScalarValue::Float(crate::types::numeric::i64_to_f64(
            *value,
        ))),
        _ => cast_scalar(value, &DataType::Float, None),
    }
}
