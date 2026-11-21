use super::accounted_page::{decode_expansion_factor, provisional_document_bytes};
use super::{
    check_controls, key_encoding, AccountedDocument, CassieError, DocumentRef, Midge,
    MidgeRowCursor,
};
use crate::midge::adapter::column_store::column_payload_logical_len;
use crate::midge::adapter::column_store::validation::{
    decode_field, decode_marker, field_is_required, missing_field,
};
use crate::runtime::QueryExecutionControls;

impl MidgeRowCursor {
    pub(super) fn decode_column_store_document(
        &self,
        midge: &Midge,
        raw_key: &[u8],
        raw_marker: &[u8],
        controls: &QueryExecutionControls,
    ) -> Result<Option<AccountedDocument>, CassieError> {
        check_controls(controls)?;
        // A marker contains no field payload. Account the row and output names first, then
        // grow the reservation from each fetched compact field before decoding that field.
        let base_bytes = provisional_document_bytes(
            &self.row_schema,
            self.projection.as_ref(),
            false,
            raw_key.len(),
            0,
        )?;
        let mut memory = controls.reserve_query_memory(base_bytes)?;
        let Some(id) = key_encoding::utf8_suffix_after_prefix(raw_key, &self.prefix) else {
            return Ok(None);
        };
        if id.is_empty() {
            return Ok(None);
        }
        let marker_version = decode_marker(&self.collection, &id, raw_marker)?;
        let mut object = serde_json::Map::new();
        let counters = midge.column_field_reads();
        for field in self.column_fields.fields(&self.row_schema) {
            check_controls(controls)?;
            // The field key adds one numeric component to the marker key. Sixteen bytes
            // bounds the encoded u32 component, separator and discriminator difference.
            let key_bytes = raw_key
                .len()
                .checked_add(16)
                .ok_or_else(accounting_overflow)?;
            let _key_memory = controls.reserve_query_memory(key_bytes)?;
            let Some(raw) = self
                .tx
                .get(&Midge::column_store_field_key(
                    self.row_schema.relation_id,
                    field.field_id,
                    &id,
                ))
                .map_err(CassieError::from)?
            else {
                if field_is_required(field, marker_version) {
                    return Err(missing_field(&self.collection, &id, field));
                }
                continue;
            };
            counters.record_payload(raw.len());
            let logical_len = column_payload_logical_len(&raw);
            check_controls(controls)?;
            let field_bytes = if matches!(
                field.data_type,
                crate::types::DataType::Json | crate::types::DataType::Jsonb
            ) {
                crate::runtime::accounted::json::serialized_decode_bytes(logical_len)?
            } else {
                logical_len
                    .checked_mul(decode_expansion_factor(&field.data_type))
                    .ok_or_else(accounting_overflow)?
            };
            memory.try_grow(field_bytes)?;
            counters.record_decode();
            let Some(value) = decode_field(&self.collection, &id, field, &raw)? else {
                continue;
            };
            counters.record_retained(field_bytes);
            check_controls(controls)?;
            object.insert(field.name.clone(), value);
        }
        check_controls(controls)?;
        midge.column_vector_counters().record_row_document();
        AccountedDocument::from_reserved_decoded_document(
            DocumentRef {
                id,
                payload: serde_json::Value::Object(object),
            },
            memory,
        )
        .map(Some)
    }
}

fn accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("controlled ColumnStore field accounting overflow".to_owned())
}
