//! Accounted blocking primitive tuple keys and stable relational selection.
use std::mem::size_of;

use super::{check_controls, invalid, owner_bytes, Cell, QueryError, TypedBatch};
use crate::executor::retained_memory::{add, mul};
use crate::executor::semantic::SemanticValue;
use crate::runtime::accounted::Accounted;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::SetOperator;
use crate::types::{DataType, Value};

pub(crate) fn native_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Null
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Boolean
    )
}

pub(crate) struct TupleKeys {
    pub(crate) values: Vec<Vec<SemanticValue>>,
    _memory: QueryMemoryReservation,
}

impl TupleKeys {
    pub(crate) fn new(
        controls: &QueryExecutionControls,
        batch: &TypedBatch,
        columns: &[usize],
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        if columns.iter().any(|index| {
            batch
                .schema()
                .get(*index)
                .is_none_or(|(_, data_type)| !native_type(data_type))
        }) {
            return Err(invalid("tuple keys require selected primitive columns"));
        }
        let row_bytes = add(
            size_of::<Vec<SemanticValue>>(),
            mul(columns.len(), size_of::<SemanticValue>())?,
        )?;
        let float_scratch = if columns
            .iter()
            .any(|column| batch.schema()[*column].1 == DataType::Float)
        {
            64
        } else {
            0
        };
        let memory = controls.reserve_query_memory(add(
            mul(batch.len(), add(row_bytes, 2 * size_of::<usize>())?)?,
            float_scratch,
        )?)?;
        let mut values = Vec::with_capacity(batch.len());
        for lane in 0..batch.len() {
            check_controls(controls)?;
            let mut key = Vec::with_capacity(columns.len());
            for column in columns {
                let value = match batch.cell(*column, lane)? {
                    Cell::Null => SemanticValue::Null,
                    Cell::Integer(value) => SemanticValue::from_value(&Value::Int64(value)),
                    Cell::Float(value) => SemanticValue::from_value(&Value::Float64(value)),
                    Cell::Boolean(value) => SemanticValue::Bool(value),
                    Cell::Scalar(value)
                        if matches!(
                            value,
                            Value::Null | Value::Int64(_) | Value::Float64(_) | Value::Bool(_)
                        ) =>
                    {
                        SemanticValue::from_value(value)
                    }
                    _ => {
                        return Err(invalid(
                            "native tuple cell disagrees with its primitive type",
                        ))
                    }
                };
                key.push(value);
            }
            values.push(key);
        }
        Ok(Self {
            values,
            _memory: memory,
        })
    }

    pub(crate) fn scalar_rows(
        controls: &QueryExecutionControls,
        rows: &[crate::executor::batch::BatchRow],
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let bytes = rows.iter().try_fold(
            mul(
                rows.len(),
                add(size_of::<Vec<SemanticValue>>(), 2 * size_of::<usize>())?,
            )?,
            |bytes, row| {
                row.entries().iter().try_fold(bytes, |bytes, (_, value)| {
                    add(
                        bytes,
                        add(size_of::<SemanticValue>(), scalar_heap_bytes(value)?)?,
                    )
                })
            },
        )?;
        let float_scratch = if rows.iter().any(|row| {
            row.entries()
                .iter()
                .any(|(_, value)| matches!(value, Value::Float64(_)))
        }) {
            64
        } else {
            0
        };
        let memory = controls.reserve_query_memory(add(bytes, float_scratch)?)?;
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            check_controls(controls)?;
            let mut tuple = Vec::with_capacity(row.entries().len());
            for (_, value) in row.entries() {
                check_controls(controls)?;
                tuple.push(SemanticValue::from_value(value));
            }
            values.push(tuple);
        }
        Ok(Self {
            values,
            _memory: memory,
        })
    }

    pub(crate) fn scalar_columns(
        controls: &QueryExecutionControls,
        rows: &[crate::executor::batch::BatchRow],
        columns: &[usize],
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let mut bytes = mul(
            rows.len(),
            add(size_of::<Vec<SemanticValue>>(), 2 * size_of::<usize>())?,
        )?;
        let mut float_scratch = 0;
        for row in rows {
            check_controls(controls)?;
            for &column in columns {
                let value = &row
                    .entries()
                    .get(column)
                    .ok_or_else(|| invalid("direct tuple column missing"))?
                    .1;
                bytes = add(
                    bytes,
                    add(size_of::<SemanticValue>(), scalar_heap_bytes(value)?)?,
                )?;
                if matches!(value, Value::Float64(_)) {
                    float_scratch = 64;
                }
            }
        }
        let memory = controls.reserve_query_memory(add(bytes, float_scratch)?)?;
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            check_controls(controls)?;
            let mut tuple = Vec::with_capacity(columns.len());
            for &column in columns {
                check_controls(controls)?;
                tuple.push(SemanticValue::from_value(&row.entries()[column].1));
            }
            values.push(tuple);
        }
        Ok(Self {
            values,
            _memory: memory,
        })
    }

    pub(crate) fn distinct_positions(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<usize>, QueryError> {
        let mut positions = (0..self.values.len()).collect::<Vec<_>>();
        positions.sort_unstable_by(|left, right| {
            self.values[*left]
                .cmp(&self.values[*right])
                .then_with(|| left.cmp(right))
        });
        check_controls(controls)?;
        positions.dedup_by(|right, left| self.values[*right] == self.values[*left]);
        positions.sort_unstable();
        Ok(positions)
    }
}

pub(crate) fn set_positions(
    controls: &QueryExecutionControls,
    left_keys: &TupleKeys,
    right_keys: &TupleKeys,
    operator: SetOperator,
) -> Result<Accounted<Vec<(bool, usize)>>, QueryError> {
    check_controls(controls)?;
    let left_len = left_keys.values.len();
    let right_len = right_keys.values.len();
    let total = add(left_len, right_len)?;
    let memory = controls.reserve_query_memory(owner_bytes::<Vec<(bool, usize)>>(mul(
        total,
        size_of::<(bool, usize)>(),
    )?)?)?;
    let mut right_positions = (0..right_len).collect::<Vec<_>>();
    right_positions
        .sort_unstable_by(|left, right| right_keys.values[*left].cmp(&right_keys.values[*right]));
    let contains = |key: &Vec<SemanticValue>| {
        right_positions
            .binary_search_by(|index| right_keys.values[*index].cmp(key))
            .is_ok()
    };
    let mut output = Vec::with_capacity(total);
    for lane in 0..left_len {
        check_controls(controls)?;
        let include = match operator {
            SetOperator::Intersect => contains(&left_keys.values[lane]),
            SetOperator::Except => !contains(&left_keys.values[lane]),
            SetOperator::Union | SetOperator::UnionAll => true,
        };
        if include {
            output.push((false, lane));
        }
    }
    if matches!(operator, SetOperator::Union | SetOperator::UnionAll) {
        output.extend((0..right_len).map(|lane| (true, lane)));
    }
    let key = |entry: &(bool, usize)| {
        if entry.0 {
            &right_keys.values[entry.1]
        } else {
            &left_keys.values[entry.1]
        }
    };
    output.sort_unstable_by(|left, right| key(left).cmp(key(right)).then_with(|| left.cmp(right)));
    check_controls(controls)?;
    if !matches!(operator, SetOperator::UnionAll) {
        output.dedup_by(|right, left| key(right) == key(left));
    }
    Ok(super::admitted(output, memory))
}

fn scalar_heap_bytes(value: &Value) -> Result<usize, crate::app::CassieError> {
    match value {
        Value::String(text) => mul(text.len().max(27), 2),
        Value::Vector(vector) => mul(vector.values.len(), size_of::<u32>()),
        Value::Json(value) => Ok(mul(
            crate::executor::retained_memory::serialized_json_bytes(value)?,
            2,
        )?
        .max(128)),
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => Ok(0),
    }
}
