use super::super::{CassieError, Midge};
use super::{
    DocumentWriteBatchOptions, DocumentWriteBatchReport, DocumentWriteOp, ReplayBatchCommit,
};

impl Midge {
    pub(crate) fn apply_projection_replay_batch(
        &self,
        collection: &str,
        operations: Vec<DocumentWriteOp>,
        replay: ReplayBatchCommit,
        commit: cntryl_midge::WriteOptions,
        rollup_change_fields: Vec<String>,
        record_rollup_maintenance_debt: bool,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        let mut options = DocumentWriteBatchOptions::sync(commit);
        options.replay = Some(replay);
        options.rollup_change_fields = rollup_change_fields;
        options.record_rollup_maintenance_debt = record_rollup_maintenance_debt;
        self.apply_document_write_batch_with_options(collection, operations, &options)
    }
}
