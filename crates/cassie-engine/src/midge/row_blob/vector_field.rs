pub(crate) use cassie_storage::row_blob::{PackedVector, VectorColumn};

pub(crate) fn split_vector_row<'a>(
    schema: &super::RowSchema,
    column: &VectorColumn,
    row: &'a [u8],
    with_payload: bool,
) -> Result<cassie_storage::row_blob::VectorRowParts<'a>, crate::app::CassieError> {
    cassie_storage::row_blob::split_vector_row(
        schema,
        column,
        row,
        with_payload,
        super::decode_engine_json,
    )
    .map_err(super::map_row_decode_error)
}

#[cfg(test)]
#[path = "vector_field_tests.rs"]
mod tests;
