#[derive(Debug, Default)]
pub(crate) struct DocumentWriteBatchReport {
    pub ids: Vec<String>,
    pub changed_ids: Vec<String>,
    pub row_delta: i64,
    pub stats: crate::runtime::ProjectionWriteStats,
    pub data_epoch: Option<u64>,
}

impl DocumentWriteBatchReport {
    pub(super) fn has_changes(&self) -> bool {
        self.stats.row_puts > 0
            || self.stats.row_deletes > 0
            || self.stats.index_puts > 0
            || self.stats.index_deletes > 0
            || self.stats.metadata_puts > 0
            || self.stats.metadata_deletes > 0
    }
}
