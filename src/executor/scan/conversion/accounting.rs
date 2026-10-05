use std::mem::size_of;

use super::{check_controls, InputBatch, Request, Shape};
use crate::app::CassieError;
use crate::catalog::CollectionSchema;
use crate::executor::batch::{Batch, BatchRow};
use crate::executor::QueryError;
use crate::midge::adapter::DocumentRef;
use crate::runtime::accounted::json;
use crate::types::row_identity::{
    is_identity_reference, is_row_identity_column, ROW_IDENTITY_COLUMN,
};
use crate::types::{DataType, Value};

pub(super) fn identifier_scratch(
    inputs: &[InputBatch],
    request: &Request<'_>,
    workers: usize,
) -> Result<usize, CassieError> {
    if inputs.iter().all(|input| input.len() == 0) {
        return Ok(0);
    }
    let schema_has_id = request.schema.is_some_and(CollectionSchema::declares_id);
    let mut longest = match request.shape {
        Shape::Full => 0,
        Shape::Projected { fields, filter } => fields
            .iter()
            .filter(|field| !is_identity_reference(field, schema_has_id))
            .map(String::len)
            .chain(filter.map(|filter| filter.field.len()))
            .max()
            .unwrap_or(0),
    };
    if has_array_metadata(request.schema) {
        longest = longest.max(ROW_IDENTITY_COLUMN.len());
        if let Some(schema) = request.schema {
            longest = longest.max(
                schema
                    .fields
                    .iter()
                    .map(|field| field.name.len())
                    .max()
                    .unwrap_or(0),
            );
        }
        for input in inputs {
            for index in 0..input.len() {
                if let Some(object) = input.document(index).payload.as_object() {
                    longest = longest.max(object.keys().map(String::len).max().unwrap_or(0));
                }
            }
        }
    }
    if longest == 0 {
        return Ok(0);
    }
    let catalog_name = request.schema.map_or(0, |schema| {
        schema
            .fields
            .iter()
            .map(|field| field.name.len())
            .max()
            .unwrap_or(0)
    });
    // At most four parsed components, temporary lookup candidates, escaped canonical names,
    // and one matching catalog component coexist. This allowance is reused per worker.
    mul(
        add(512, add(mul(longest, 32)?, mul(catalog_name, 8)?)?)?,
        workers.max(1),
    )
}

pub(super) fn conversion_bytes(
    inputs: &[InputBatch],
    request: &Request<'_>,
    workers: usize,
) -> Result<usize, QueryError> {
    let mut bytes = mul(inputs.len(), size_of::<Batch>())?;
    if workers > 1 {
        // Partition slots, retained worker results, joined results, final batch slots, and
        // scoped handle/header buffers. All vectors use their known exact upper lengths.
        bytes = add(bytes, mul(inputs.len(), size_of::<(usize, InputBatch)>())?)?;
        bytes = add(bytes, mul(inputs.len(), 2 * size_of::<(usize, Batch)>())?)?;
        bytes = add(bytes, mul(workers, 3 * size_of::<Vec<usize>>() + 64)?)?;
    }
    for input in inputs {
        for index in 0..input.len() {
            check_controls(request.controls)?;
            bytes = add(
                bytes,
                row_bytes(input.document(index), request.schema, request.shape)?,
            )?;
        }
    }
    Ok(bytes)
}

pub(super) fn projected_row_bytes(
    document: &DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> Result<usize, CassieError> {
    row_bytes(
        document,
        schema,
        Shape::Projected {
            fields,
            filter: None,
        },
    )
}

fn row_bytes(
    document: &DocumentRef,
    schema: Option<&CollectionSchema>,
    shape: Shape<'_>,
) -> Result<usize, CassieError> {
    let mut entries = EntryBytes::new(document)?;
    let capacity =
        match shape {
            Shape::Full => {
                full_entries(&mut entries, document, schema)?;
                grown_capacity(entries.count, 4)?
            }
            Shape::Projected { fields, filter } => {
                projected_entries(&mut entries, document, fields, schema)?;
                if let Some(filter) = filter {
                    if let Some(value) = document.payload.as_object().and_then(|object| {
                        super::super::projected_field_value(object, &filter.field)
                    }) {
                        entries.heap = add(entries.heap, value_heap(value, None)?)?;
                    }
                }
                add(fields.len(), 2)?
            }
        };
    let mut bytes = add(
        size_of::<BatchRow>(),
        mul(capacity, size_of::<(String, Value)>())?,
    )?;
    bytes = add(bytes, entries.heap)?;
    if matches!(shape, Shape::Full) {
        bytes = add(bytes, hash_table_bytes::<(String, usize)>(entries.count)?)?;
        bytes = add(bytes, entries.names)?;
        if let Some(schema) = schema.filter(|_| document.payload.is_object()) {
            let fields = schema
                .fields
                .iter()
                .filter(|field| !is_row_identity_column(&field.name));
            let mut seen_count = 0;
            for field in fields {
                seen_count = add(seen_count, 1)?;
                bytes = add(bytes, field.name.len())?;
            }
            bytes = add(bytes, hash_table_bytes::<String>(seen_count)?)?;
        }
    }
    if has_array_metadata(schema) {
        bytes = add(bytes, size_of::<Vec<DataType>>() + 2 * size_of::<usize>())?;
        bytes = add(bytes, mul(entries.count, size_of::<DataType>())?)?;
        bytes = add(bytes, mul(entries.count, maximum_type_heap(schema)?)?)?;
    }
    Ok(bytes)
}

struct EntryBytes {
    count: usize,
    names: usize,
    heap: usize,
}

impl EntryBytes {
    fn new(document: &DocumentRef) -> Result<Self, CassieError> {
        Ok(Self {
            count: 1,
            names: ROW_IDENTITY_COLUMN.len(),
            heap: add(ROW_IDENTITY_COLUMN.len(), document.id.len())?,
        })
    }

    fn push(
        &mut self,
        name: &str,
        value: Option<&serde_json::Value>,
        data_type: Option<&DataType>,
    ) -> Result<(), CassieError> {
        self.count = add(self.count, 1)?;
        self.names = add(self.names, name.len())?;
        self.heap = add(self.heap, name.len())?;
        if let Some(value) = value {
            self.heap = add(self.heap, value_heap(value, data_type)?)?;
        }
        Ok(())
    }
}

fn full_entries(
    entries: &mut EntryBytes,
    document: &DocumentRef,
    schema: Option<&CollectionSchema>,
) -> Result<(), CassieError> {
    let Some(object) = document.payload.as_object() else {
        return Ok(());
    };
    if let Some(schema) = schema {
        for field in &schema.fields {
            if !is_row_identity_column(&field.name) {
                entries.push(&field.name, object.get(&field.name), Some(&field.data_type))?;
            }
        }
    }
    for (name, value) in object {
        if is_identity_reference(name, schema.is_some_and(CollectionSchema::declares_id))
            || schema.is_some_and(|schema| schema.fields.iter().any(|field| field.name == *name))
        {
            continue;
        }
        entries.push(name, Some(value), None)?;
    }
    Ok(())
}

fn projected_entries(
    entries: &mut EntryBytes,
    document: &DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> Result<(), CassieError> {
    for field in fields {
        if is_identity_reference(field, schema.is_some_and(CollectionSchema::declares_id)) {
            continue;
        }
        let value = document
            .payload
            .as_object()
            .and_then(|object| super::super::projected_field_value(object, field));
        let data_type = super::super::field_data_type(schema, field);
        entries.push(field, value, data_type)?;
    }
    Ok(())
}

pub(super) fn value_heap(
    value: &serde_json::Value,
    data_type: Option<&DataType>,
) -> Result<usize, CassieError> {
    if let Some(DataType::Vector(dimensions)) = data_type {
        if value
            .as_array()
            .is_some_and(|values| values.len() == *dimensions)
        {
            let float_bytes = mul(grown_capacity(*dimensions, 4)?, size_of::<f32>())?;
            // Failed element parsing falls back to a complete JSON clone after the partial
            // float buffer is dropped; the numeric formatting scratch is transient.
            // Finite binary64 fixed decimal text fits within 350 bytes, including sign,
            // exponent-range zero padding and seventeen significant digits. String growth
            // can retain nearly twice that length; charge its buffer separately from values.
            return add(float_bytes.max(json::retained_bytes(value)?), 1_024);
        }
    }
    if matches!(data_type, Some(DataType::Json)) && (value.is_null() || value.is_string()) {
        return json::retained_bytes(value);
    }
    match value {
        serde_json::Value::String(value) => Ok(value.len()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => json::retained_bytes(value),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            Ok(0)
        }
    }
}

fn has_array_metadata(schema: Option<&CollectionSchema>) -> bool {
    schema.is_some_and(|schema| {
        schema
            .fields
            .iter()
            .any(|field| matches!(field.data_type, DataType::Array(_)))
    })
}

fn maximum_type_heap(schema: Option<&CollectionSchema>) -> Result<usize, CassieError> {
    schema.map_or(Ok(0), |schema| {
        schema.fields.iter().try_fold(0, |maximum, field| {
            Ok(maximum.max(type_heap(&field.data_type)?))
        })
    })
}

fn type_heap(data_type: &DataType) -> Result<usize, CassieError> {
    match data_type {
        DataType::Array(element) => add(size_of::<DataType>(), type_heap(element)?),
        _ => Ok(0),
    }
}

pub(super) fn hash_table_bytes<T>(entries: usize) -> Result<usize, CassieError> {
    if entries == 0 {
        return Ok(0);
    }
    let required = add(mul(entries, 8)?, 6)? / 7;
    let buckets = required
        .max(4)
        .checked_next_power_of_two()
        .ok_or_else(overflow)?;
    add(mul(buckets, size_of::<T>() + 1)?, 16 * size_of::<usize>())
}

fn grown_capacity(count: usize, minimum: usize) -> Result<usize, CassieError> {
    if count == 0 {
        return Ok(0);
    }
    count
        .max(minimum)
        .checked_next_power_of_two()
        .ok_or_else(overflow)
}

pub(super) fn add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(overflow)
}

pub(super) fn mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(overflow)
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("scan row conversion accounting overflow".to_owned())
}
