//! Known CTE NULL templates own their copied fields through join-template lifetime.
use std::mem::size_of;
use std::sync::Arc;

use crate::executor::retained_memory::{add, data_type_clone_bytes, lookup_bytes, mul};
use crate::runtime::QueryMemoryReservation;
use crate::types::{DataType, FieldSchema};

use super::super::{check_timeout, qualify_row, source_collection};
use super::{BatchRow, QueryError, SourceExecutionEnv, Value};

pub(super) fn known_cte_template(
    env: &SourceExecutionEnv<'_>,
    fields: Vec<FieldSchema>,
    mut memory: QueryMemoryReservation,
    qualifier: &str,
) -> Result<BatchRow, QueryError> {
    check_timeout(env.controls)?;
    let names = fields
        .iter()
        .try_fold(0, |bytes, field| add(bytes, field.name.len()))?;
    let mut bytes = add(
        mul(fields.len(), size_of::<(String, Value)>())?,
        add(
            lookup_bytes(fields.len(), names)?,
            size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
        )?,
    )?;
    bytes = add(
        bytes,
        source_collection::qualification_bytes(qualifier, fields.len(), names, 0, 0)?,
    )?;
    let arrays = fields
        .iter()
        .any(|field| matches!(field.data_type, DataType::Array(_)));
    if arrays {
        bytes = add(
            bytes,
            add(
                size_of::<Vec<DataType>>() + 2 * size_of::<usize>(),
                mul(fields.len(), size_of::<DataType>())?,
            )?,
        )?;
        for field in &fields {
            bytes = add(bytes, data_type_clone_bytes(&field.data_type)?)?;
        }
    }
    memory.try_grow(bytes)?;
    let _scratch = source_collection::qualification_scratch(env, qualifier)?;
    let memory = Arc::new(memory);
    let data_types = if arrays {
        let mut types = Vec::new();
        types
            .try_reserve_exact(fields.len())
            .map_err(|error| allocation(&error))?;
        for field in &fields {
            check_timeout(env.controls)?;
            types.push(field.data_type.clone());
        }
        Some(Arc::new(types))
    } else {
        None
    };
    let mut values = Vec::new();
    values
        .try_reserve_exact(fields.len())
        .map_err(|error| allocation(&error))?;
    for field in fields {
        check_timeout(env.controls)?;
        values.push((field.name, Value::Null));
    }
    let row = BatchRow::new(values)
        .with_optional_data_types(data_types)
        .with_query_memory(Some(Arc::clone(&memory)));
    let row = qualify_row(row, qualifier).retain_operator_memory(env.controls, memory)?;
    check_timeout(env.controls)?;
    Ok(row)
}

fn allocation(error: &std::collections::TryReserveError) -> QueryError {
    crate::app::CassieError::ResourceLimit(format!("unable to retain CTE template: {error}")).into()
}
