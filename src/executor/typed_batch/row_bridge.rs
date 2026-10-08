//! Primitive physical key carriers inferred without altering SQL row descriptors.
use std::mem::size_of;

use super::{check_controls, relational::native_type, QueryError, TypedBatch};
use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

pub(crate) fn from_rows(
    controls: &QueryExecutionControls,
    rows: &[BatchRow],
    columns: &[usize],
) -> Result<Option<TypedBatch>, QueryError> {
    check_controls(controls)?;
    if controls.uses_relational_cte_boundary() {
        return Ok(None);
    }
    // Complete eligibility without allocating or consuming input. Mixed physical variants decline
    // rather than coercing exact integers through FLOAT; each set branch infers independently.
    for column in columns {
        if infer_column(controls, rows, *column)?.is_none() {
            return Ok(None);
        }
    }
    let inline = add(size_of::<Vec<Value>>(), size_of::<(String, DataType)>())?;
    let bytes = mul(
        columns.len(),
        add(inline, mul(rows.len(), size_of::<Value>())?)?,
    )?;
    let _scratch = controls.reserve_query_memory(bytes)?;
    let mut schema = Vec::with_capacity(columns.len());
    let mut values = Vec::with_capacity(columns.len());
    for column in columns {
        let data_type = infer_column(controls, rows, *column)?
            .expect("eligibility was established without modifying input");
        schema.push((String::new(), data_type));
        let mut cells = Vec::with_capacity(rows.len());
        for row in rows {
            check_controls(controls)?;
            cells.push(row.entries()[*column].1.clone());
        }
        values.push(cells);
    }
    TypedBatch::from_columns(controls, &schema, &values, rows.len(), None).map(Some)
}

fn infer_column(
    controls: &QueryExecutionControls,
    rows: &[BatchRow],
    column: usize,
) -> Result<Option<DataType>, QueryError> {
    let mut inferred = DataType::Null;
    let mut declared = None;
    for row in rows {
        check_controls(controls)?;
        let Some((_, value)) = row.entries().get(column) else {
            return Ok(None);
        };
        if let Some(data_type) = row.data_types().get(column) {
            if !native_type(data_type)
                || declared
                    .as_ref()
                    .is_some_and(|previous| previous != data_type)
            {
                return Ok(None);
            }
            declared = Some(data_type.clone());
        }
        let carrier = match value {
            Value::Null => continue,
            Value::Int64(_) => DataType::BigInt,
            Value::Float64(_) => DataType::Float,
            Value::Bool(_) => DataType::Boolean,
            Value::String(_) | Value::Vector(_) | Value::Json(_) => return Ok(None),
        };
        if inferred != DataType::Null && inferred != carrier {
            return Ok(None);
        }
        inferred = carrier;
    }
    if let Some(declared) = declared {
        let compatible = inferred == DataType::Null
            || match (&declared, &inferred) {
                (DataType::SmallInt | DataType::Int | DataType::BigInt, DataType::BigInt) => true,
                _ => declared == inferred,
            };
        return Ok(compatible.then_some(declared));
    }
    Ok(Some(inferred))
}
