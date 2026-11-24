//! Row-order aggregate accumulator for filtered column-batch scans.
//!
//! Errors use [`CassieError::Execution`] so an overflow or a non-numeric
//! input is classified exactly like the executor's own aggregate errors.

use super::{CassieError, ColumnBatchAggregateSpec};
use crate::types::extended::numeric::{math as numeric_math, Numeric};
use crate::types::numeric::{self, i64_to_f64, usize_to_f64};
use crate::types::semantic::compare_values;
use crate::types::typed::TypedValue;
use crate::types::Value;

fn exact_numeric(total: i128) -> Value {
    Value::Typed(TypedValue::Numeric(Numeric::from_i128(total)))
}

pub(super) enum DirectAggregateAccumulator {
    Count(i64),
    Sum {
        value: Option<Value>,
        seen: bool,
    },
    /// `exact` totals integer inputs; `avg(integer)` is a `numeric` quotient.
    Avg {
        sum: f64,
        count: usize,
        exact: i128,
        decimal: Numeric,
        float: bool,
    },
    Min {
        value: Option<Value>,
        max: bool,
    },
}

impl DirectAggregateAccumulator {
    pub(super) fn new(spec: &ColumnBatchAggregateSpec) -> Self {
        match spec.function.as_str() {
            "count" => Self::Count(0),
            "sum" => Self::Sum {
                value: None,
                seen: false,
            },
            "avg" => Self::Avg {
                sum: 0.0,
                count: 0,
                exact: 0,
                decimal: Numeric::zero(),
                float: false,
            },
            "max" => Self::Min {
                value: None,
                max: true,
            },
            _ => Self::Min {
                value: None,
                max: false,
            },
        }
    }

    pub(super) fn update_count(&mut self, rows: usize) -> Result<(), CassieError> {
        let Self::Count(count) = self else {
            return Ok(());
        };
        *count = count
            .checked_add(
                i64::try_from(rows).map_err(|_| {
                    CassieError::Execution("aggregate row count overflow".to_string())
                })?,
            )
            .ok_or_else(|| CassieError::Execution("aggregate row count overflow".to_string()))?;
        Ok(())
    }

    pub(super) fn update_values(
        &mut self,
        spec: &ColumnBatchAggregateSpec,
        values: &[Value],
    ) -> Result<(), CassieError> {
        match self {
            Self::Count(count) => {
                *count = count
                    .checked_add(
                        i64::try_from(values.iter().filter(|v| !v.is_null()).count()).map_err(
                            |_| CassieError::Execution("aggregate row count overflow".to_string()),
                        )?,
                    )
                    .ok_or_else(|| {
                        CassieError::Execution("aggregate row count overflow".to_string())
                    })?;
            }
            Self::Sum { value, seen } => {
                for current in values.iter().filter(|value| !value.is_null()) {
                    fold_sum(value, current, spec)?;
                    *seen = true;
                }
            }
            Self::Avg {
                sum,
                count,
                exact,
                decimal,
                float,
            } => {
                for current in values {
                    match current {
                        Value::Typed(TypedValue::Numeric(value)) => {
                            *decimal = add_numeric(decimal, value)?;
                            add_float(sum, value.to_f64())?;
                            *count = count.checked_add(1).ok_or_else(|| {
                                CassieError::Execution("aggregate row count overflow".to_string())
                            })?;
                        }
                        Value::Int64(value) => {
                            *exact += i128::from(*value);
                            add_float(sum, i64_to_f64(*value))?;
                            *count = count.checked_add(1).ok_or_else(|| {
                                CassieError::Execution("aggregate row count overflow".to_string())
                            })?;
                        }
                        Value::Float64(value) => {
                            *float = true;
                            add_float(sum, *value)?;
                            *count = count.checked_add(1).ok_or_else(|| {
                                CassieError::Execution("aggregate row count overflow".to_string())
                            })?;
                        }
                        Value::Null => {}
                        other => {
                            return Err(CassieError::Execution(
                                numeric::non_numeric_aggregate_input("avg", other),
                            ))
                        }
                    }
                }
            }
            Self::Min { value, max } => {
                for current in values.iter().filter(|value| !value.is_null()) {
                    let replace = value.as_ref().is_none_or(|selected| {
                        let ordering = compare_values(current, selected);
                        if *max {
                            ordering.is_gt()
                        } else {
                            ordering.is_lt()
                        }
                    });
                    if replace {
                        *value = Some(current.clone());
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        match self {
            Self::Count(count) => Value::Int64(count),
            Self::Sum { value, seen } => {
                if seen {
                    value.unwrap_or(Value::Null)
                } else {
                    Value::Null
                }
            }
            Self::Avg {
                sum,
                count,
                exact,
                decimal,
                float,
            } => {
                if count == 0 {
                    Value::Null
                } else if float {
                    Value::Float64(sum / usize_to_f64(count))
                } else {
                    let count = Numeric::from_i128(i128::try_from(count).unwrap_or(i128::MAX));
                    numeric_math::add(&decimal, &Numeric::from_i128(exact))
                        .and_then(|total| numeric_math::divide(&total, &count))
                        .map_or(Value::Null, |mean| Value::Typed(TypedValue::Numeric(mean)))
                }
            }
            Self::Min { value, .. } => value.unwrap_or(Value::Null),
        }
    }
}

/// Folds one non-NULL input into a `sum` total; `sum(int8)` is `numeric`.
fn fold_sum(
    value: &mut Option<Value>,
    current: &Value,
    spec: &ColumnBatchAggregateSpec,
) -> Result<(), CassieError> {
    match current {
        Value::Int64(next) => match value {
            None => *value = Some(Value::Int64(*next)),
            Some(Value::Int64(total)) => {
                // `sum(int8)` is numeric, so the total may leave i64.
                if let Some(sum) = total.checked_add(*next) {
                    *total = sum;
                } else {
                    let sum = i128::from(*total) + i128::from(*next);
                    *value = Some(exact_numeric(sum));
                }
            }
            Some(Value::Typed(TypedValue::Numeric(total))) => {
                *total = numeric_math::add(total, &Numeric::from_i64(*next))
                    .map_err(|error| CassieError::Execution(error.message))?;
            }
            Some(Value::Float64(total)) => add_float(total, i64_to_f64(*next))?,
            _ => {
                return Err(CassieError::Execution(
                    "unsupported aggregate type".to_string(),
                ))
            }
        },
        Value::Typed(TypedValue::Numeric(next)) => {
            *value = Some(match value.take() {
                None => Value::Typed(TypedValue::Numeric(next.clone())),
                Some(Value::Int64(total)) => Value::Typed(TypedValue::Numeric(add_numeric(
                    &Numeric::from_i64(total),
                    next,
                )?)),
                Some(Value::Typed(TypedValue::Numeric(total))) => {
                    Value::Typed(TypedValue::Numeric(add_numeric(&total, next)?))
                }
                Some(Value::Float64(mut total)) => {
                    add_float(&mut total, next.to_f64())?;
                    Value::Float64(total)
                }
                Some(_) => {
                    return Err(CassieError::Execution(
                        "unsupported aggregate type".to_string(),
                    ))
                }
            });
        }
        Value::Float64(next) => {
            if value.is_none() {
                *value = Some(Value::Float64(0.0));
            }
            if let Some(Value::Int64(total)) = value {
                *value = Some(Value::Float64(i64_to_f64(*total)));
            }
            if let Some(Value::Typed(TypedValue::Numeric(total))) = value {
                *value = Some(Value::Float64(total.to_f64()));
            }
            if let Some(Value::Float64(total)) = value {
                add_float(total, *next)?;
            }
        }
        other => {
            return Err(CassieError::Execution(
                numeric::non_numeric_aggregate_input(&spec.function, other),
            ))
        }
    }
    Ok(())
}

fn add_numeric(total: &Numeric, value: &Numeric) -> Result<Numeric, CassieError> {
    numeric_math::add(total, value).map_err(|error| CassieError::Execution(error.message))
}

/// Adds `value` to `sum`, rejecting finite operands that overflow to an
/// infinity the way the executor's SUM/AVG do.
fn add_float(sum: &mut f64, value: f64) -> Result<(), CassieError> {
    if numeric::add_f64_overflowed(sum, value) {
        return Err(CassieError::Execution(numeric::FLOAT_OVERFLOW.to_string()));
    }
    Ok(())
}
