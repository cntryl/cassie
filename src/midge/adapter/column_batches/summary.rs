use std::collections::{BTreeMap, BTreeSet};

use crate::app::CassieError;
use crate::catalog::{ColumnBatchFieldSummary, ColumnBatchNumericSum, ColumnBatchRow};
use crate::midge::row_blob::RowSchema;
use crate::types::semantic::compare_values;
use crate::types::{DataType, Value, Vector};

use super::{checksum_hex, CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION};

pub(super) fn column_values(
    payload: &serde_json::Value,
    fields: &[String],
) -> BTreeMap<String, serde_json::Value> {
    let object = payload.as_object();
    fields
        .iter()
        .map(|field| {
            let value = object
                .and_then(|object| {
                    object.get(field).or_else(|| {
                        object
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(field))
                            .map(|(_, value)| value)
                    })
                })
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            (field.clone(), value)
        })
        .collect()
}

pub(super) fn column_batch_summaries(
    rows: &[ColumnBatchRow],
    fields: &[String],
    row_schema: &RowSchema,
) -> BTreeMap<String, ColumnBatchFieldSummary> {
    fields
        .iter()
        .map(|field| {
            let data_type = row_schema
                .fields
                .iter()
                .find(|candidate| {
                    !candidate.retired
                        && (candidate.name.eq_ignore_ascii_case(field)
                            || candidate
                                .aliases
                                .iter()
                                .any(|alias| alias.eq_ignore_ascii_case(field)))
                })
                .map(|candidate| &candidate.data_type);
            (
                field.clone(),
                column_batch_field_summary(rows, field, data_type),
            )
        })
        .collect()
}

pub(super) fn summary_checksum(
    row_count: usize,
    summaries: &BTreeMap<String, ColumnBatchFieldSummary>,
) -> Result<String, CassieError> {
    let bytes = serde_json::to_vec(&(
        CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION,
        row_count,
        summaries,
    ))
    .map_err(|error| CassieError::Parse(format!("serialize column summary: {error}")))?;
    Ok(checksum_hex(&bytes))
}

pub(super) fn json_to_typed_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    Value::Json(value.clone())
}

fn json_to_schema_value(value: &serde_json::Value, data_type: Option<&DataType>) -> Value {
    if let Some(DataType::Vector(dimensions)) = data_type {
        if let Some(values) = value.as_array() {
            let vector = (values.len() == *dimensions)
                .then(|| {
                    values
                        .iter()
                        .map(|part| part.as_f64()?.to_string().parse::<f32>().ok())
                        .collect::<Option<Vec<_>>>()
                })
                .flatten();
            if let Some(vector) = vector {
                return Value::Vector(Vector::new(vector));
            }
        }
    }
    json_to_typed_value(value)
}

pub(super) fn compare_summary_to_json(
    summary: &Value,
    value: &serde_json::Value,
) -> std::cmp::Ordering {
    compare_values(summary, &json_to_typed_value(value))
}

fn column_batch_field_summary(
    rows: &[ColumnBatchRow],
    field: &str,
    data_type: Option<&DataType>,
) -> ColumnBatchFieldSummary {
    let mut non_null_count = 0usize;
    let mut numeric_count = 0usize;
    let mut min: Option<Value> = None;
    let mut max: Option<Value> = None;
    let mut sum = NumericSummaryAccumulator::default();
    let mut avg_sum = 0.0_f64;
    let mut distinct = BTreeSet::new();

    for row in rows {
        let value = row
            .values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map_or(&serde_json::Value::Null, |(_, value)| value);
        if value.is_null() {
            continue;
        }
        non_null_count += 1;
        distinct.insert(value.to_string());
        let typed = json_to_schema_value(value, data_type);
        if min
            .as_ref()
            .is_none_or(|current| compare_values(&typed, current).is_lt())
        {
            min = Some(typed.clone());
        }
        if max
            .as_ref()
            .is_none_or(|current| compare_values(&typed, current).is_gt())
        {
            max = Some(typed.clone());
        }
        match typed {
            Value::Int64(value) => {
                sum.add_int(value);
                avg_sum += i64_to_f64(value);
                numeric_count += 1;
            }
            Value::Float64(value) => {
                sum.add_float(value);
                avg_sum += value;
                numeric_count += 1;
            }
            Value::Null => {}
            Value::Bool(_) | Value::String(_) | Value::Vector(_) | Value::Json(_) => {
                sum.promote_to_float();
            }
        }
    }

    let numeric = sum.finish();
    ColumnBatchFieldSummary {
        non_null_count,
        numeric_count,
        min,
        max,
        sum: numeric.sum,
        integer_total: numeric.integer_total,
        integer_prefix_min: numeric.integer_prefix_min,
        integer_prefix_max: numeric.integer_prefix_max,
        avg_sum: (numeric_count > 0).then_some(avg_sum),
        distinct_hint: Some(distinct.len()),
    }
}

struct NumericSummaryAccumulator {
    state: NumericSummaryState,
    seen: bool,
    integer_total: Option<i128>,
    integer_prefix_min: Option<i128>,
    integer_prefix_max: Option<i128>,
}

impl Default for NumericSummaryAccumulator {
    fn default() -> Self {
        Self {
            state: NumericSummaryState::default(),
            seen: false,
            integer_total: Some(0),
            integer_prefix_min: None,
            integer_prefix_max: None,
        }
    }
}

#[derive(Default)]
enum NumericSummaryState {
    #[default]
    IntegerZero,
    Integer(i64),
    Float(f64),
    IntegerOverflow,
}

impl NumericSummaryAccumulator {
    fn add_int(&mut self, value: i64) {
        self.seen = true;
        if let Some(total) = self.integer_total {
            self.integer_total = total.checked_add(i128::from(value));
            if let Some(total) = self.integer_total {
                self.integer_prefix_min = Some(
                    self.integer_prefix_min
                        .map_or(total, |minimum| minimum.min(total)),
                );
                self.integer_prefix_max = Some(
                    self.integer_prefix_max
                        .map_or(total, |maximum| maximum.max(total)),
                );
            } else {
                self.integer_prefix_min = None;
                self.integer_prefix_max = None;
            }
        }
        self.state = match self.state {
            NumericSummaryState::IntegerZero => NumericSummaryState::Integer(value),
            NumericSummaryState::Integer(sum) => sum.checked_add(value).map_or(
                NumericSummaryState::IntegerOverflow,
                NumericSummaryState::Integer,
            ),
            NumericSummaryState::Float(sum) => NumericSummaryState::Float(sum + i64_to_f64(value)),
            NumericSummaryState::IntegerOverflow => NumericSummaryState::IntegerOverflow,
        };
    }

    fn add_float(&mut self, value: f64) {
        self.seen = true;
        self.promote_to_float();
        if let NumericSummaryState::Float(sum) = &mut self.state {
            *sum += value;
        }
    }

    fn promote_to_float(&mut self) {
        self.integer_total = None;
        self.integer_prefix_min = None;
        self.integer_prefix_max = None;
        self.state = match self.state {
            NumericSummaryState::IntegerZero => NumericSummaryState::Float(0.0),
            NumericSummaryState::Integer(sum) => NumericSummaryState::Float(i64_to_f64(sum)),
            NumericSummaryState::Float(sum) => NumericSummaryState::Float(sum),
            NumericSummaryState::IntegerOverflow => NumericSummaryState::IntegerOverflow,
        };
    }

    fn finish(self) -> NumericSummary {
        if !self.seen {
            let sum = match self.state {
                NumericSummaryState::Float(_) => ColumnBatchNumericSum::FloatEmpty,
                NumericSummaryState::IntegerZero
                | NumericSummaryState::Integer(_)
                | NumericSummaryState::IntegerOverflow => ColumnBatchNumericSum::Empty,
            };
            return NumericSummary {
                sum,
                integer_total: None,
                integer_prefix_min: None,
                integer_prefix_max: None,
            };
        }
        let sum = match self.state {
            NumericSummaryState::IntegerZero => ColumnBatchNumericSum::Integer(0),
            NumericSummaryState::Integer(sum) => ColumnBatchNumericSum::Integer(sum),
            NumericSummaryState::Float(sum) => ColumnBatchNumericSum::Float(sum),
            NumericSummaryState::IntegerOverflow => ColumnBatchNumericSum::IntegerOverflow,
        };
        NumericSummary {
            sum,
            integer_total: self.integer_total,
            integer_prefix_min: self.integer_prefix_min,
            integer_prefix_max: self.integer_prefix_max,
        }
    }
}

struct NumericSummary {
    sum: ColumnBatchNumericSum,
    integer_total: Option<i128>,
    integer_prefix_min: Option<i128>,
    integer_prefix_max: Option<i128>,
}

fn i64_to_f64(value: i64) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("i64 should convert to finite f64")
}
