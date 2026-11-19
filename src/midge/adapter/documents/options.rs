use super::super::WriteOptions;

#[derive(Debug, Clone)]
pub(crate) struct DocumentWriteBatchOptions {
    pub(crate) commit: WriteOptions,
    pub(crate) data_database: Option<String>,
    pub(crate) additional_data_writes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    pub(crate) rollup_change_fields: Vec<String>,
    pub(crate) publication_marker: Option<(Vec<u8>, Vec<u8>)>,
    pub(crate) refresh_after_commit: bool,
    pub(crate) normalized_vector_collection: Option<String>,
    pub(crate) record_rollup_maintenance_debt: bool,
    pub(crate) record_materialized_projection_maintenance_debt: bool,
    pub(super) replay: Option<super::super::projections::ReplayBatchCommit>,
}

impl DocumentWriteBatchOptions {
    pub(crate) fn sync(commit: WriteOptions) -> Self {
        Self {
            commit,
            data_database: None,
            additional_data_writes: Vec::new(),
            rollup_change_fields: Vec::new(),
            publication_marker: None,
            refresh_after_commit: true,
            normalized_vector_collection: None,
            record_rollup_maintenance_debt: false,
            record_materialized_projection_maintenance_debt: false,
            replay: None,
        }
    }

    pub(crate) fn buffered(commit: WriteOptions) -> Self {
        Self {
            commit,
            data_database: None,
            additional_data_writes: Vec::new(),
            rollup_change_fields: Vec::new(),
            publication_marker: None,
            refresh_after_commit: true,
            normalized_vector_collection: None,
            record_rollup_maintenance_debt: false,
            record_materialized_projection_maintenance_debt: false,
            replay: None,
        }
    }
}
