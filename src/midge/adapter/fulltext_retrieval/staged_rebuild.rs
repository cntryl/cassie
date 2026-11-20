//! Full-text rebuilds published in bounded storage transactions.
//!
//! A whole index does not fit one storage transaction once it grows past a few
//! megabytes, so a rebuild is staged: the metadata record is deleted first
//! (readers then take the labelled exact fallback), the old artifacts are
//! deleted and the new ones written in bounded batches, and one final
//! transaction publishes the metadata record together with anything the caller
//! must publish atomically with it. A crash before that final transaction
//! leaves an index without metadata, which startup reconciliation rebuilds.

use super::super::schema_write_control::{pause_before_schema_write_commit, SchemaWritePausePoint};
use super::{
    build_state, fulltext_build_ranges, CassieError, FulltextWriteTally, IndexMeta, Midge,
    FULLTEXT_INDEX_BUILD_WRITE_BATCH_SIZE,
};
use crate::midge::adapter::AdapterTransaction;

/// Largest rebuilt index, in key plus value bytes, that a document commit
/// writes inside its own transaction. A larger one is rebuilt after the commit.
const FULLTEXT_IN_COMMIT_REBUILD_MAX_BYTES: usize = 1024 * 1024;

fn record_bytes(records: &[(Vec<u8>, Vec<u8>)]) -> Vec<usize> {
    records
        .iter()
        .map(|(key, value)| key.len().saturating_add(value.len()))
        .collect()
}

impl Midge {
    /// Rebuilds `index` from committed rows in bounded transactions. `finalize`
    /// stages writes into the final transaction, which also publishes the
    /// metadata record; `pause` is reached before that transaction commits.
    ///
    /// The caller holds the collection write gate when concurrent document
    /// writers must not interleave.
    pub(super) fn rebuild_fulltext_index_staged(
        &self,
        index: &IndexMeta,
        finalize: impl FnOnce(&mut AdapterTransaction) -> Result<(), CassieError>,
        pause: Option<SchemaWritePausePoint>,
    ) -> Result<FulltextWriteTally, CassieError> {
        let collection = self.canonical_collection_name(&index.collection);
        let generation = self.collection_generation(&collection)?;
        let row_schema = self.row_schema(&collection)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let documents = self.load_documents_from_tx(&tx, &collection, &row_schema)?;
        drop(tx);
        let state = build_state(index, generation, documents)?;
        let records = Self::fulltext_state_records(index, &state)?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
        let mut tally = FulltextWriteTally {
            full_rebuild: true,
            ..FulltextWriteTally::default()
        };

        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        tx.delete(Self::fulltext_index_key(relation_id, index_id))
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        let prefix = Self::fulltext_index_artifact_prefix(relation_id, index_id);
        tally.pieces_deleted = self.delete_fulltext_artifacts_in_batches(&collection, &prefix)?;

        for range in fulltext_build_ranges(&record_bytes(&records.body)) {
            let mut tx = self.begin_data_rw_tx_for(&collection)?;
            for (key, value) in &records.body[range] {
                tally.pieces_written = tally.pieces_written.saturating_add(1);
                tally.bytes_written = tally
                    .bytes_written
                    .saturating_add(key.len().saturating_add(value.len()));
                tx.put(key.clone(), value.clone(), None)
                    .map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }

        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        let (metadata_key, metadata_value) = records.metadata;
        tally.pieces_written = tally.pieces_written.saturating_add(1);
        tally.bytes_written = tally
            .bytes_written
            .saturating_add(metadata_key.len().saturating_add(metadata_value.len()));
        tx.put(metadata_key, metadata_value, None)
            .map_err(CassieError::from)?;
        finalize(&mut tx)?;
        if let Some(pause) = pause {
            pause_before_schema_write_commit(pause);
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(tally)
    }

    fn delete_fulltext_artifacts_in_batches(
        &self,
        collection: &str,
        prefix: &[u8],
    ) -> Result<usize, CassieError> {
        let mut deleted = 0_usize;
        loop {
            let keys = self
                .raw_scan_prefix_page_for_collection(
                    collection,
                    prefix,
                    FULLTEXT_INDEX_BUILD_WRITE_BATCH_SIZE,
                )?
                .into_iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>();
            if keys.is_empty() {
                return Ok(deleted);
            }
            deleted = deleted.saturating_add(keys.len());
            let mut tx = self.begin_data_rw_tx_for(collection)?;
            for key in keys {
                tx.delete(key).map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }
    }

    /// Repairs an index that a document commit cannot maintain incrementally.
    ///
    /// A rebuilt index small enough for the commit transaction is written
    /// there. A larger one is unpublished and recorded as maintenance debt in
    /// the commit, so the write still succeeds; the returned flag tells the
    /// caller to rebuild it once the commit is durable.
    pub(super) fn rebuild_or_defer_fulltext_index_in_tx(
        &self,
        tx: &mut AdapterTransaction,
        collection: &str,
        index: &IndexMeta,
        generation: u64,
    ) -> Result<(FulltextWriteTally, bool), CassieError> {
        let row_schema = self.row_schema(collection)?;
        let documents = self.load_documents_from_tx(tx, collection, &row_schema)?;
        let state = build_state(index, generation, documents)?;
        let records = Self::fulltext_state_records(index, &state)?;
        let body_bytes = record_bytes(&records.body).into_iter().sum::<usize>();
        let mut tally = FulltextWriteTally {
            full_rebuild: true,
            ..FulltextWriteTally::default()
        };
        if body_bytes > FULLTEXT_IN_COMMIT_REBUILD_MAX_BYTES {
            let (relation_id, index_id) = Self::fulltext_storage_ids(index)?;
            tx.delete(Self::fulltext_index_key(relation_id, index_id))
                .map_err(CassieError::from)?;
            Self::record_fulltext_maintenance_debt_in_tx(tx, collection, &index.name, generation)?;
            return Ok((FulltextWriteTally::default(), true));
        }
        tally.pieces_deleted = Self::delete_fulltext_artifacts_in_tx(tx, index)?;
        for (key, value) in std::iter::once(records.metadata).chain(records.body) {
            tally.pieces_written = tally.pieces_written.saturating_add(1);
            tally.bytes_written = tally
                .bytes_written
                .saturating_add(key.len().saturating_add(value.len()));
            tx.put(key, value, None).map_err(CassieError::from)?;
        }
        Ok((tally, false))
    }
}
