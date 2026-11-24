use crate::adapter::error::StorageError;
use crate::adapter::{ColumnBatchScanFallbackReason, StorageRequestContext};

use super::{EncodedSourceDecision, Midge};

impl Midge {
    /// Replaces one field chunk and its manifest for cross-crate adapter fixtures.
    #[doc(hidden)]
    ///
    /// # Errors
    ///
    /// Returns an error if the requested chunk is absent or the fixture write fails.
    pub fn replace_column_batch_chunk_for_test(
        &self,
        index: &cassie_sql::catalog_declarations::IndexMeta,
        metadata: &cassie_core::catalog::ColumnBatchMetadata,
        field: &str,
        raw: Vec<u8>,
        segment_position: usize,
    ) -> Result<(), StorageError> {
        let (relation_id, index_id) = Self::column_batch_storage_ids(index)?;
        let segment = metadata.segments.get(segment_position).ok_or_else(|| {
            StorageError::InvalidQuery("column batch fixture segment is missing".to_string())
        })?;
        if !segment.field_chunks.contains_key(field) {
            return Err(StorageError::InvalidQuery(
                "column batch fixture field chunk is missing".to_string(),
            ));
        }

        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        tx.put(
            Self::column_batch_field_key(
                relation_id,
                index_id,
                segment.segment_id,
                segment.revision,
                field,
            ),
            raw,
            None,
        )
        .map_err(StorageError::from)?;
        tx.put(
            Self::column_batch_metadata_key(relation_id, index_id),
            crate::column_batch_format_v1::encode_manifest(metadata)?,
            None,
        )
        .map_err(StorageError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(StorageError::from)
    }

    /// Returns compact diagnostics from the existing validated encoded-source path.
    #[doc(hidden)]
    ///
    /// # Errors
    ///
    /// Returns an error if storage access or request-control validation fails.
    pub fn diagnose_controlled_encoded_source_for_test(
        &self,
        collection: &str,
        fields: &[String],
        controls: &StorageRequestContext<'_>,
    ) -> Result<
        (
            bool,
            Option<ColumnBatchScanFallbackReason>,
            usize,
            Option<bool>,
            usize,
        ),
        StorageError,
    > {
        match self.controlled_encoded_source(collection, fields, controls)? {
            EncodedSourceDecision::Hit(source) => {
                let first_field = source
                    .segments
                    .first()
                    .and_then(|segment| segment.fields.first())
                    .map(|field| field.get());
                Ok((
                    true,
                    None,
                    source.metrics.materialized_values,
                    first_field.map(|field| field.boolean_values().is_some()),
                    first_field.map_or(0, |field| field.values().len()),
                ))
            }
            EncodedSourceDecision::Fallback(reason) => {
                Ok((false, Some(reason), 0, None, 0))
            }
            EncodedSourceDecision::TransportBudget => Ok((false, None, 0, None, 0)),
        }
    }
}
