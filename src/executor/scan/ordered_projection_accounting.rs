use std::mem::size_of;

use crate::app::CassieError;
use crate::catalog::CollectionSchema;
use crate::executor::batch::BatchRow;
use crate::midge::adapter::DocumentRef;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

use super::conversion;

pub(crate) struct OrderedProjectionShape {
    pub(crate) bytes: usize,
    pub(crate) order_value_bytes: usize,
    _scratch_memory: QueryMemoryReservation,
}

pub(crate) fn ordered_projection_shape<'a>(
    document: &DocumentRef,
    mut projection: impl Iterator<Item = (&'a str, &'a str)> + Clone,
    schema: Option<&CollectionSchema>,
    order_column: Option<&str>,
    controls: &QueryExecutionControls,
) -> Result<OrderedProjectionShape, CassieError> {
    let count = projection.clone().count();
    let field_names = projection
        .clone()
        .try_fold(0, |bytes, (field, _)| add(bytes, field.len()))?;
    let output_names = projection
        .clone()
        .try_fold(0, |bytes, (_, output)| add(bytes, output.len()))?;
    let descriptor_bytes = add(
        size_of::<Vec<String>>(),
        add(mul(count, size_of::<String>())?, field_names)?,
    )?;
    let longest = projection
        .clone()
        .map(|(field, _)| field.len())
        .chain(order_column.map(str::len))
        .max()
        .unwrap_or(0)
        .max(crate::types::row_identity::ROW_IDENTITY_COLUMN.len());
    let catalog_name = schema.map_or(0, |schema| {
        schema
            .fields
            .iter()
            .map(|field| field.name.len())
            .max()
            .unwrap_or(0)
    });
    // Identifier parsing, lookup candidates and canonical field matching use the same
    // bounded scratch geometry as controlled row conversion. Reserve before parsing.
    let scratch = add(512, add(mul(longest, 32)?, mul(catalog_name, 8)?)?)?;
    let scratch_memory = controls.reserve_query_memory(add(descriptor_bytes, scratch)?)?;
    let mut fields = Vec::with_capacity(count);
    fields.extend(projection.clone().map(|(field, _)| field.to_owned()));

    let temporary = add(
        conversion::projected_row_bytes(document, &fields, schema)?,
        conversion::projected_lookup_bytes(&fields, schema)?,
    )?;
    let order_value_bytes = order_column.map_or(Ok(0), |field| {
        conversion::projected_value_heap(document, field, schema)
    })?;
    let final_values = projection.try_fold(0, |bytes, (field, _)| {
        add(
            bytes,
            conversion::projected_value_heap(document, field, schema)?,
        )
    })?;
    let final_row = add(
        size_of::<BatchRow>(),
        add(
            mul(count, size_of::<(String, Value)>())?,
            add(final_values, output_names)?,
        )?,
    )?;
    // The temporary row initializes its lazy lookup during get(). A retained final
    // row can also initialize its lookup, including the column-heap's BatchRow::new.
    let final_lookup = conversion::lookup_bytes(count, output_names)?;
    Ok(OrderedProjectionShape {
        bytes: add(temporary, add(final_row, final_lookup)?)?,
        order_value_bytes,
        _scratch_memory: scratch_memory,
    })
}

fn add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(overflow)
}

fn mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(overflow)
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("ordered projection accounting overflow".to_owned())
}
