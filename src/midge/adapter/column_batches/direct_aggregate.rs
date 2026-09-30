//! Row-order aggregate accumulator for filtered column-batch scans.
//!
//! Errors use [`CassieError::Execution`] so an overflow or a non-numeric
//! input is classified exactly like the executor's own aggregate errors.

use super::{CassieError, ColumnBatchAggregateSpec};
use crate::types::numeric::{self, i64_to_f64, usize_to_f64};
use crate::types::semantic::compare_values;
use crate::types::Value;

pub(super) enum DirectAggregateAccumulator {
    Count(i64),
    Sum { value: Option<Value>, seen: bool },
    Avg { sum: f64, count: usize },
    Min { value: Option<Value>, max: bool },
}

impl DirectAggregateAccumulator {
    pub(super) fn new(spec: &ColumnBatchAggregateSpec) -> Self {
        match spec.function.as_str() {
            "count" => Self::Count(0),
            "sum" => Self::Sum {
                value: None,
                seen: false,
            },
            "avg" => Self::Avg { sum: 0.0, count: 0 },
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
                    match current {
                        Value::Int64(next) => match value {
                            None => *value = Some(Value::Int64(*next)),
                            Some(Value::Int64(total)) => {
                                *total = total.checked_add(*next).ok_or_else(|| {
                                    CassieError::Execution("aggregate integer overflow".to_string())
                                })?;
                            }
                            Some(Value::Float64(total)) => add_float(total, i64_to_f64(*next))?,
                            _ => {
                                return Err(CassieError::Execution(
                                    "unsupported aggregate type".to_string(),
                                ))
                            }
                        },
                        Value::Float64(next) => {
                            if value.is_none() {
                                *value = Some(Value::Float64(0.0));
                            }
                            if let Some(Value::Int64(total)) = value {
                                *value = Some(Value::Float64(i64_to_f64(*total)));
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
                    *seen = true;
                }
            }
            Self::Avg { sum, count } => {
                for current in values {
                    match current {
                        Value::Int64(value) => {
                            add_float(sum, i64_to_f64(*value))?;
                            *count = count.checked_add(1).ok_or_else(|| {
                                CassieError::Execution("aggregate row count overflow".to_string())
                            })?;
                        }
                        Value::Float64(value) => {
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
            Self::Avg { sum, count } => {
                if count == 0 {
                    Value::Null
                } else {
                    Value::Float64(sum / usize_to_f64(count))
                }
            }
            Self::Min { value, .. } => value.unwrap_or(Value::Null),
        }
    }
}

/// Adds `value` to `sum`, rejecting finite operands that overflow to an
/// infinity the way the executor's SUM/AVG do.
fn add_float(sum: &mut f64, value: f64) -> Result<(), CassieError> {
    if numeric::add_f64_overflowed(sum, value) {
        return Err(CassieError::Execution(numeric::FLOAT_OVERFLOW.to_string()));
    }
    Ok(())
}
