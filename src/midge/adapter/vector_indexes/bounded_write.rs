//! Byte-bounded, publish-last writes for vector index sidecars.
//!
//! Midge validates each detached transaction against an 8 MiB budget that
//! charges 512 bytes plus key and value size per operation. Rebuilds therefore
//! stage normalized vectors, graph nodes and list memberships in transactions
//! bounded by bytes (not only by row count), diff against the persisted
//! artifacts so unchanged records are not rewritten, and publish the state
//! manifest in the final transaction. Readers that observe a partially staged
//! rebuild see a manifest that no longer matches its artifacts and take a
//! labelled exact fallback; the manifest never points at unstaged artifacts.

use std::collections::BTreeMap;
use std::time::Instant;

use super::codec::{encode_hnsw_node, encode_normalized_vector};
use super::{
    check_document_write_failure_point, collect_scan, CassieError, DocumentWriteFailurePoint,
    Midge, NormalizedVectorRecord, Query, VectorIndexState,
};
use crate::midge::adapter::schema_write_control::{
    pause_before_schema_write_commit, SchemaWritePausePoint,
};

const SIDECAR_TX_BYTE_BUDGET: usize = 4 * 1024 * 1024;
const SIDECAR_TX_OP_LIMIT: usize = 5_000;
const OP_OVERHEAD_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SidecarOp {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
}

impl SidecarOp {
    fn cost(&self) -> usize {
        OP_OVERHEAD_BYTES
            + match self {
                Self::Put(key, value) => key.len().saturating_add(value.len()),
                Self::Delete(key) => key.len(),
            }
    }
}

/// Splits `ops` into transactions bounded by estimated bytes and operation
/// count. `final_ops` always land together in the last transaction.
pub(super) fn chunk_sidecar_ops(
    ops: Vec<SidecarOp>,
    final_ops: Vec<SidecarOp>,
    byte_budget: usize,
    max_ops: usize,
) -> Vec<Vec<SidecarOp>> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut cost = 0usize;
    for op in ops {
        let op_cost = op.cost();
        if !current.is_empty()
            && (cost.saturating_add(op_cost) > byte_budget || current.len() >= max_ops)
        {
            chunks.push(std::mem::take(&mut current));
            cost = 0;
        }
        cost = cost.saturating_add(op_cost);
        current.push(op);
    }
    let final_cost = final_ops.iter().map(SidecarOp::cost).sum::<usize>();
    if !current.is_empty()
        && !final_ops.is_empty()
        && (cost.saturating_add(final_cost) > byte_budget
            || current.len().saturating_add(final_ops.len()) > max_ops)
    {
        chunks.push(std::mem::take(&mut current));
    }
    current.extend(final_ops);
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

impl Midge {
    /// Scans `prefix` and returns the deletes and puts that make it equal
    /// `desired`. Records whose persisted value already matches are skipped.
    fn diff_sidecar_prefix(
        &self,
        collection: &str,
        prefix: Vec<u8>,
        desired: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(Vec<SidecarOp>, Vec<SidecarOp>), CassieError> {
        let tx = self.begin_data_readonly_tx_for(collection)?;
        let mut existing = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        drop(tx);
        let mut puts = Vec::new();
        for (key, value) in desired {
            if existing.remove(&key).as_ref() != Some(&value) {
                puts.push(SidecarOp::Put(key, value));
            }
        }
        let deletes = existing.into_keys().map(SidecarOp::Delete).collect();
        Ok((deletes, puts))
    }

    /// Rebuilds persisted vector artifacts for one index.
    ///
    /// `records` selects the normalized-vector sidecars (`None` leaves them
    /// untouched). `state` carries the graph or list membership artifacts and
    /// is published last, together with the source summary. Without a state
    /// only the normalized vectors are reconciled.
    pub(super) fn publish_vector_sidecars(
        &self,
        collection: &str,
        field: &str,
        records: Option<&[NormalizedVectorRecord]>,
        state: Option<VectorIndexState>,
    ) -> Result<(), CassieError> {
        let write_gate = self.collection_write_gate(collection);
        let _write_guard = write_gate.lock();
        let started = Instant::now();
        let rebuild = state
            .is_some()
            .then(|| self.vector_index_maintenance_metrics.begin_rebuild());
        let generation = self.collection_generation(collection)?;
        let (relation_id, field_id) = self.vector_storage_ids(collection, field)?;
        let mut deletes = Vec::new();
        let mut puts = Vec::new();
        if let Some(records) = records {
            let mut desired = Vec::with_capacity(records.len());
            for record in records {
                let mut record = record.clone();
                record.built_generation = generation;
                desired.push((
                    Self::normalized_vector_key(relation_id, field_id, &record.id),
                    encode_normalized_vector(&record)?,
                ));
            }
            let (stale, changed) = self.diff_sidecar_prefix(
                collection,
                Self::normalized_vector_prefix(relation_id, field_id),
                desired,
            )?;
            deletes.extend(stale);
            puts.extend(changed);
        }
        let mut final_ops = Vec::new();
        if let Some(mut state) = state {
            state.built_generation = generation;
            let mut nodes = Vec::new();
            if let Some(graph) = &state.hnsw_graph {
                for node in &graph.nodes {
                    nodes.push((
                        super::super::key_encoding::hnsw_graph_node_key(
                            relation_id,
                            field_id,
                            &node.id,
                        ),
                        encode_hnsw_node(node)?,
                    ));
                }
            }
            let mut memberships = Vec::new();
            if let Some(training) = &state.ivfflat_training {
                for (id, list) in &training.assignments {
                    memberships.push((
                        super::super::key_encoding::ivfflat_membership_key(
                            relation_id,
                            field_id,
                            *list,
                            id,
                        ),
                        Vec::new(),
                    ));
                }
            }
            for (prefix, desired) in [
                (
                    super::super::key_encoding::hnsw_graph_node_prefix(relation_id, field_id),
                    nodes,
                ),
                (
                    super::super::key_encoding::ivfflat_membership_prefix(relation_id, field_id),
                    memberships,
                ),
            ] {
                let (stale, changed) = self.diff_sidecar_prefix(collection, prefix, desired)?;
                deletes.extend(stale);
                puts.extend(changed);
            }
            let source = state
                .hnsw_graph
                .as_ref()
                .map(|graph| (graph.source_fingerprint, graph.row_count))
                .or_else(|| {
                    state
                        .ivfflat_training
                        .as_ref()
                        .map(|training| (training.source_fingerprint, training.row_count))
                });
            if let Some((source_fingerprint, row_count)) = source {
                let summary = super::vector_retrieval::HnswSourceSummary {
                    built_generation: generation,
                    source_fingerprint,
                    row_count,
                };
                final_ops.push(SidecarOp::Put(
                    super::super::key_encoding::ivfflat_source_summary_key(relation_id, field_id),
                    serde_json::to_vec(&summary)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
                ));
            }
            let (manifest_key, manifest_value) =
                Self::vector_index_manifest_entry(relation_id, field_id, &state)?;
            final_ops.push(SidecarOp::Put(manifest_key, manifest_value));
        }
        deletes.extend(puts);
        self.commit_sidecar_ops(collection, deletes, final_ops)?;
        if let Some(start) = rebuild {
            self.vector_index_maintenance_metrics
                .finish_rebuild(start, started.elapsed());
        }
        Ok(())
    }

    fn commit_sidecar_ops(
        &self,
        collection: &str,
        ops: Vec<SidecarOp>,
        final_ops: Vec<SidecarOp>,
    ) -> Result<(), CassieError> {
        let publishes = !final_ops.is_empty();
        let chunks = chunk_sidecar_ops(ops, final_ops, SIDECAR_TX_BYTE_BUDGET, SIDECAR_TX_OP_LIMIT);
        let last = chunks.len().saturating_sub(1);
        for (index, chunk) in chunks.into_iter().enumerate() {
            let bytes = chunk.iter().map(SidecarOp::cost).sum::<usize>();
            let mut tx = self.begin_data_rw_tx_for(collection)?;
            for op in chunk {
                match op {
                    SidecarOp::Put(key, value) => {
                        tx.put(key, value, None).map_err(CassieError::from)?;
                    }
                    SidecarOp::Delete(key) => tx.delete(key).map_err(CassieError::from)?,
                }
            }
            let publishing = publishes && index == last;
            if publishing {
                check_document_write_failure_point(DocumentWriteFailurePoint::VectorState)?;
                pause_before_schema_write_commit(SchemaWritePausePoint::VectorIndexRebuildCommit);
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
            self.vector_index_maintenance_metrics
                .record_transaction(bytes);
            if publishing {
                check_document_write_failure_point(
                    DocumentWriteFailurePoint::VectorStatePublished,
                )?;
            }
        }
        Ok(())
    }

    /// Vector rebuild cost counters merged into the `vector` runtime metrics.
    #[must_use]
    pub fn vector_index_maintenance_metrics(&self) -> serde_json::Value {
        self.vector_index_maintenance_metrics.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(size: usize) -> SidecarOp {
        SidecarOp::Put(vec![1], vec![0; size])
    }

    #[test]
    fn should_bound_every_sidecar_transaction_by_bytes() {
        // Arrange
        let ops = (0..10).map(|_| put(1_000)).collect::<Vec<_>>();

        // Act
        let chunks = chunk_sidecar_ops(ops, Vec::new(), 4_000, 5_000);

        // Assert
        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), [2; 5]);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.iter().map(SidecarOp::cost).sum::<usize>() <= 4_000));
    }

    #[test]
    fn should_keep_final_operations_together_in_the_last_transaction() {
        // Arrange
        let ops = vec![put(1_000), put(1_000)];
        let final_ops = vec![put(1_000), put(1_000)];

        // Act
        let chunks = chunk_sidecar_ops(ops, final_ops.clone(), 4_000, 5_000);

        // Assert
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1], final_ops);
    }

    #[test]
    fn should_isolate_one_oversized_operation_in_its_own_transaction() {
        // Arrange
        let ops = vec![put(10), put(100_000), put(10)];

        // Act
        let chunks = chunk_sidecar_ops(ops, Vec::new(), 4_000, 5_000);

        // Assert
        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 1]);
    }

    #[test]
    fn should_chunk_nothing_when_there_is_no_work() {
        // Arrange / Act
        let chunks = chunk_sidecar_ops(Vec::new(), Vec::new(), 4_000, 5_000);

        // Assert
        assert!(chunks.is_empty());
    }
}
