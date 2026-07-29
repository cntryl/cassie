use super::super::WriteOptions;

#[derive(Debug, Clone)]
pub(crate) struct DocumentWriteBatchOptions {
    pub(crate) commit: WriteOptions,
    pub(crate) refresh_after_commit: bool,
    pub(crate) normalized_vector_collection: Option<String>,
    pub(crate) record_rollup_maintenance_debt: bool,
    pub(crate) record_materialized_projection_maintenance_debt: bool,
}

impl DocumentWriteBatchOptions {
    pub(crate) fn sync(commit: WriteOptions) -> Self {
        Self {
            commit,
            refresh_after_commit: true,
            normalized_vector_collection: None,
            record_rollup_maintenance_debt: false,
            record_materialized_projection_maintenance_debt: false,
        }
    }

    pub(crate) fn buffered(commit: WriteOptions) -> Self {
        Self {
            commit,
            refresh_after_commit: true,
            normalized_vector_collection: None,
            record_rollup_maintenance_debt: false,
            record_materialized_projection_maintenance_debt: false,
        }
    }
}
