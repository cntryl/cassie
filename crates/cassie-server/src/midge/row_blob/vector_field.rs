//! Direct `f32` decoding of a stored vector field.
//!
//! The generic row decoder turns every vector element into a JSON number, which costs far more
//! than the four stored bytes per component. Exact vector search only needs the numbers, so this
//! module reads the packed components of one `VECTOR` field straight into an `f32` buffer and
//! decodes the remaining fields (when a filter needs them) without the vector.

use super::{
    active_field_names, field_bit, field_matches_name, DataType, RowDirectory, RowFieldMeta,
    RowSchema, TYPE_VECTOR_F32,
};
use crate::app::CassieError;

/// A stored vector field of a row schema, located by name.
#[derive(Debug, Clone)]
pub(crate) struct VectorColumn {
    field_index: usize,
}

/// The packed components of one stored vector.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PackedVector<'a> {
    components: &'a [u8],
}

/// One row split into its vector and, when requested, its other fields.
pub(crate) struct VectorRowParts<'a> {
    /// `None` when the row has no vector value (absent or SQL NULL).
    pub(crate) vector: Option<PackedVector<'a>>,
    /// The row's other active fields, decoded as the generic decoder would.
    pub(crate) payload: Option<serde_json::Value>,
}

impl VectorColumn {
    /// Locates the `VECTOR` field named `field_name`, or `None` when the schema has no such
    /// field so the caller keeps the generic decoder.
    pub(crate) fn resolve(schema: &RowSchema, field_name: &str) -> Option<Self> {
        let requested = crate::sql::ColumnIdentifierPath::from_field_name(field_name).lookup_key();
        let active = active_field_names(schema);
        let field_index = schema
            .fields
            .iter()
            .position(|field| field_matches_name(field, &requested, true, &active))?;
        matches!(schema.fields[field_index].data_type, DataType::Vector(_))
            .then_some(Self { field_index })
    }
}

impl PackedVector<'_> {
    /// Number of components.
    pub(crate) const fn dimensions(&self) -> usize {
        self.components.len() / 4
    }

    /// Bytes the decoded `f32` buffer occupies.
    pub(crate) const fn decoded_bytes(&self) -> usize {
        self.components.len()
    }

    /// Reads the components into an `f32` buffer.
    pub(crate) fn to_f32(self) -> Result<Vec<f32>, CassieError> {
        let mut values = Vec::with_capacity(self.dimensions());
        for chunk in self.components.as_chunks::<4>().0 {
            let value = f32::from_be_bytes(*chunk);
            // The generic decoder rejects non-finite components; keep that diagnostic.
            if !value.is_finite() {
                return Err(CassieError::Parse(
                    "invalid vector numeric value in row blob".to_string(),
                ));
            }
            values.push(value);
        }
        Ok(values)
    }
}

/// Splits `row` into the packed vector of `column` and, when `with_payload`, the other fields.
pub(crate) fn split_vector_row<'a>(
    schema: &RowSchema,
    column: &VectorColumn,
    row: &'a [u8],
    with_payload: bool,
) -> Result<VectorRowParts<'a>, CassieError> {
    let directory = RowDirectory::parse(row)?;
    let vector_field = &schema.fields[column.field_index];
    let vector = packed_vector(&directory, vector_field)?;
    let payload = if with_payload {
        let mut object = serde_json::Map::new();
        for field in &schema.fields {
            if field.retired || field.field_id == vector_field.field_id {
                continue;
            }
            if let Some(value) = directory.decode(field)? {
                object.insert(field.name.clone(), value);
            }
        }
        Some(serde_json::Value::Object(object))
    } else {
        None
    };
    Ok(VectorRowParts { vector, payload })
}

fn packed_vector<'a>(
    directory: &RowDirectory<'a>,
    field: &RowFieldMeta,
) -> Result<Option<PackedVector<'a>>, CassieError> {
    if !field_bit(directory.presence, field.field_id) || field_bit(directory.nulls, field.field_id)
    {
        return Ok(None);
    }
    let (type_tag, offset, len) = directory
        .entries
        .get(&field.field_id)
        .copied()
        .ok_or_else(|| CassieError::Parse("row blob field missing directory entry".to_string()))?;
    if type_tag != TYPE_VECTOR_F32 {
        return Err(CassieError::Parse(format!(
            "row blob field '{}' stores type tag {type_tag} but its schema declares type tag {TYPE_VECTOR_F32}",
            field.name
        )));
    }
    let bytes = &directory.payload[offset..offset + len];
    let Some((header, components)) = bytes.split_first_chunk::<4>() else {
        return Err(CassieError::Parse(
            "invalid vector field in row blob".into(),
        ));
    };
    let dimensions = usize::try_from(u32::from_be_bytes(*header))
        .map_err(|_| CassieError::Parse("invalid vector dimension in row blob".to_string()))?;
    if dimensions.checked_mul(4) != Some(components.len()) {
        return Err(CassieError::Parse(
            "invalid vector length in row blob".into(),
        ));
    }
    Ok(Some(PackedVector { components }))
}

#[cfg(test)]
#[path = "vector_field_tests.rs"]
mod tests;
