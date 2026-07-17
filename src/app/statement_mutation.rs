use std::collections::BTreeMap;

use crate::midge::adapter::DocumentWriteOp;
use crate::runtime::QueryExecutionControls;

use super::{Cassie, CassieError, CassieSession, StatementMutationBatch, TransactionRowChange};

pub(super) struct CommittedWriteBatch {
    changed_collections: Vec<(String, i64)>,
}

impl Cassie {
    pub(crate) fn new_statement_batch(
        &self,
        session: Option<&CassieSession>,
    ) -> Result<StatementMutationBatch, CassieError> {
        session.map_or_else(
            || {
                CassieSession::new("postgres".to_string(), Some(self.default_database.clone()))
                    .fork_statement_batch()
            },
            CassieSession::fork_statement_batch,
        )
    }

    pub(crate) fn publish_statement_batch(
        &self,
        original: Option<&CassieSession>,
        batch: StatementMutationBatch,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        check_publication_controls(Some(controls))?;
        if batch.has_explicit_transaction() {
            let original = original.ok_or_else(|| {
                CassieError::Execution(
                    "statement mutation lost its explicit transaction".to_string(),
                )
            })?;
            let publication = original.publish_statement_batch(&batch);
            drop(batch);
            return publication;
        }
        if original.is_some_and(|session| {
            session.is_transaction_active() || session.is_transaction_failed()
        }) {
            return Err(CassieError::Execution(
                "session transaction changed while statement was executing".to_string(),
            ));
        }

        let staging = batch.into_session();
        let committed = self.apply_staged_write_batches(&staging, Some(controls))?;
        self.finish_staged_write_batches(committed, Some(controls));
        Ok(())
    }

    pub(super) fn apply_staged_write_batches(
        &self,
        staging: &CassieSession,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<CommittedWriteBatch, CassieError> {
        let mut writes = transaction_write_batches(staging);
        if writes.is_empty() {
            check_publication_controls(controls)?;
            return Ok(CommittedWriteBatch {
                changed_collections: Vec::new(),
            });
        }

        let collections = writes.keys().cloned().collect::<Vec<_>>();
        let mut options = self.document_write_options_for_collections(&collections);
        options.refresh_after_commit = false;
        check_publication_controls(controls)?;
        let reports = match self
            .midge
            .apply_document_write_batches_with_options(&writes, &options)
        {
            Ok(reports) => reports,
            Err(CassieError::UniqueViolation { .. })
                if !staging.transaction_conflict_intents().is_empty() =>
            {
                crate::executor::resolve_transaction_conflict_intents(self, staging)
                    .map_err(CassieError::from)?;
                writes = transaction_write_batches(staging);
                if writes.is_empty() {
                    BTreeMap::new()
                } else {
                    let retry_collections = writes.keys().cloned().collect::<Vec<_>>();
                    let mut retry_options =
                        self.document_write_options_for_collections(&retry_collections);
                    retry_options.refresh_after_commit = false;
                    check_publication_controls(controls)?;
                    self.midge
                        .apply_document_write_batches_with_options(&writes, &retry_options)?
                }
            }
            Err(error) => return Err(error),
        };

        let mut changed_collections = Vec::new();
        let mut latest_epoch = None;
        for (collection, report) in reports {
            self.runtime
                .record_projection_write_batch(collection.clone(), &report.stats);
            if report_has_changes(&report.stats) {
                changed_collections.push((collection, report.row_delta));
            }
            latest_epoch = latest_epoch.max(report.data_epoch);
        }
        if let Some(epoch) = latest_epoch {
            self.runtime.set_data_epoch(epoch);
        }

        Ok(CommittedWriteBatch {
            changed_collections,
        })
    }

    pub(super) fn finish_staged_write_batches(
        &self,
        mut committed: CommittedWriteBatch,
        controls: Option<&QueryExecutionControls>,
    ) {
        committed
            .changed_collections
            .sort_by(|left, right| left.0.cmp(&right.0));
        committed
            .changed_collections
            .dedup_by(|left, right| left.0 == right.0);
        let fallback_controls = controls
            .is_none()
            .then(|| self.runtime.query_controls(std::time::Instant::now()));
        let controls = controls
            .or(fallback_controls.as_ref())
            .expect("provided or fallback query controls");
        for (collection, row_delta) in committed.changed_collections {
            let _ = self
                .midge
                .refresh_document_maintenance_after_commit(&collection, row_delta);
            let _ =
                crate::executor::refresh_rollups_for_source_external(self, &collection, controls);
            let _ = crate::executor::mark_source_projections_stale_external(self, &collection);
            let _ = crate::executor::sync_derived_maintenance_debt_external(self, &collection);
        }
    }
}

fn transaction_write_batches(session: &CassieSession) -> BTreeMap<String, Vec<DocumentWriteOp>> {
    session
        .transaction_writes()
        .into_iter()
        .filter_map(|(collection, collection_writes)| {
            let write_ops = collection_writes
                .into_iter()
                .map(|(id, change)| match change {
                    TransactionRowChange::Upsert(payload) => DocumentWriteOp::Put { id, payload },
                    TransactionRowChange::Delete => DocumentWriteOp::Delete { id },
                })
                .collect::<Vec<_>>();
            (!write_ops.is_empty()).then_some((collection, write_ops))
        })
        .collect()
}

fn check_publication_controls(
    controls: Option<&QueryExecutionControls>,
) -> Result<(), CassieError> {
    let Some(controls) = controls else {
        return Ok(());
    };
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

fn report_has_changes(stats: &crate::runtime::ProjectionWriteStats) -> bool {
    stats.row_puts > 0
        || stats.row_deletes > 0
        || stats.index_puts > 0
        || stats.index_deletes > 0
        || stats.metadata_puts > 0
        || stats.metadata_deletes > 0
        || stats.batch_flushes > 0
}
