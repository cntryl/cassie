use super::{Cassie, CassieError};
use crate::runtime::ProjectionWriteStats;

impl Cassie {
    pub(crate) fn refresh_document_write_metadata(
        &self,
        collection: &str,
        row_delta: i64,
        stats: &ProjectionWriteStats,
    ) -> Result<(), CassieError> {
        if !document_write_changed(stats) {
            return Ok(());
        }

        if self.can_increment_cardinality_after_write(collection, row_delta, stats)? {
            self.increment_cardinality_stats(collection, row_delta)?;
        } else {
            self.refresh_cardinality_stats(collection)?;
        }
        self.refresh_projection_metadata(collection)
    }

    fn can_increment_cardinality_after_write(
        &self,
        collection: &str,
        row_delta: i64,
        stats: &ProjectionWriteStats,
    ) -> Result<bool, CassieError> {
        if row_delta == 0 || stats.index_puts > 0 || stats.index_deletes > 0 {
            return Ok(false);
        }
        if !self.catalog.list_indexes(collection).is_empty()
            || !self.catalog.list_vector_indexes(collection).is_empty()
            || self.midge.collection_uses_column_store(collection)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn increment_cardinality_stats(
        &self,
        collection: &str,
        row_delta: i64,
    ) -> Result<(), CassieError> {
        let mut stats = self
            .catalog
            .get_cardinality_stats(collection)
            .unwrap_or_default();
        if row_delta.is_positive() {
            stats.row_count = stats.row_count.saturating_add(row_delta.unsigned_abs());
        } else {
            stats.row_count = stats.row_count.saturating_sub(row_delta.unsigned_abs());
        }
        stats.hydrated = true;
        stats.indexes.clear();
        stats.fields.clear();

        self.midge.save_cardinality_stats(collection, &stats)?;
        self.runtime.record_cardinality_write();
        self.catalog.hydrate_cardinality_stats(collection, stats);
        Ok(())
    }
}

fn document_write_changed(stats: &ProjectionWriteStats) -> bool {
    stats.row_puts > 0
        || stats.row_deletes > 0
        || stats.index_puts > 0
        || stats.index_deletes > 0
        || stats.metadata_puts > 0
        || stats.metadata_deletes > 0
        || stats.batch_flushes > 0
}
