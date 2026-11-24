//! Localized vector index maintenance inside a document write transaction.
//!
//! A commit that changes documents no longer rebuilds each HNSW graph. For the
//! changed documents only, it inserts new nodes, replaces nodes whose vector
//! changed and deletes removed nodes, reading just the graph nodes those
//! operations visit and writing just the nodes whose records changed plus the
//! constant-size manifest and source summary. Normalized-vector sidecars are
//! rewritten for the changed documents alone.
//!
//! A persisted graph that contradicts itself (a missing node, an unsupported
//! version, a stale generation) never fails the user's write. It is repaired by
//! a full rebuild inside the commit when the index is small, and otherwise
//! unpublished with durable maintenance debt and rebuilt right after the commit
//! (or at the next startup), so readers take the labelled exact fallback
//! meanwhile.

use std::collections::BTreeMap;

use super::codec::{decode_hnsw_node, decode_vector_index_state, encode_hnsw_node};
use super::{
    check_document_write_failure_point, vector_field_id, CassieError, DocumentWriteFailurePoint,
    Midge, NormalizedVectorRecord, VectorIndexRecord, VectorIndexState,
};
use crate::embeddings::{HnswGraphState, VectorIndexType};
use crate::vector::hnsw::{GraphMaintainer, MaintainError, MaintainerSeed, HNSW_GRAPH_VERSION};

/// Largest number of documents one commit maintains node by node. A larger
/// batch is repaired like a damaged graph, because inserting thousands of nodes
/// one by one costs more than rebuilding and cannot fit one transaction.
const INCREMENTAL_DOCUMENT_LIMIT: usize = 128;

/// Largest estimated rebuilt graph a commit writes inside its own
/// transaction. A larger one is rebuilt after the commit.
const IN_COMMIT_REBUILD_MAX_BYTES: usize = 256 * 1024;

type Deltas = [(String, Option<NormalizedVectorRecord>)];

/// Final normalized-vector state of every document one batch wrote, for one
/// indexed field. `None` means the document no longer has an eligible vector.
#[derive(Debug, Clone)]
pub(in crate::midge::adapter) struct VectorFieldWrites {
    pub(in crate::midge::adapter) field: String,
    pub(in crate::midge::adapter) deltas: Vec<(String, Option<NormalizedVectorRecord>)>,
}

/// Cost of the maintenance one commit performed for one index.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct VectorMaintenanceTally {
    pub(crate) documents: usize,
    pub(crate) nodes_read: usize,
    pub(crate) nodes_written: usize,
    pub(crate) nodes_deleted: usize,
    pub(crate) full_rebuild: bool,
}

#[derive(Debug, Clone)]
pub(in crate::midge::adapter) struct DeferredVectorRebuild {
    collection: String,
    field: String,
    generation: u64,
}

/// Maintenance a batch staged, applied to the metrics only once it commits.
#[derive(Debug, Default)]
pub(in crate::midge::adapter) struct VectorMaintenanceOutcome {
    tallies: Vec<VectorMaintenanceTally>,
    deferred: Vec<DeferredVectorRebuild>,
}

/// Vector work of one document write batch.
#[derive(Debug, Default)]
pub(in crate::midge::adapter) struct VectorBatchWork {
    pub(in crate::midge::adapter) writes_by_collection: BTreeMap<String, Vec<VectorFieldWrites>>,
    pub(in crate::midge::adapter) maintenance: VectorMaintenanceOutcome,
}

enum Incremental {
    Applied(VectorMaintenanceTally),
    Rebuild { rows_hint: Option<usize> },
}

impl Midge {
    /// Final normalized-vector state of each written document, per indexed field.
    pub(in crate::midge::adapter) fn vector_field_writes(
        indexes: &[VectorIndexRecord],
        writes: &[(String, Vec<NormalizedVectorRecord>)],
        display_collection: Option<&str>,
    ) -> Vec<VectorFieldWrites> {
        indexes
            .iter()
            .map(|index| {
                let mut finals = BTreeMap::new();
                for (id, records) in writes {
                    let record = records
                        .iter()
                        .find(|record| record.field == index.field)
                        .map(|record| {
                            let mut record = record.clone();
                            if let Some(display) = display_collection {
                                record.collection = display.to_string();
                            }
                            record
                        });
                    finals.insert(id.clone(), record);
                }
                VectorFieldWrites {
                    field: index.field.clone(),
                    deltas: finals.into_iter().collect(),
                }
            })
            .collect()
    }

    /// The complete eligible population of `index` after `deltas` apply.
    fn vector_population_after(
        &self,
        index: &VectorIndexRecord,
        deltas: &Deltas,
    ) -> Result<Vec<NormalizedVectorRecord>, CassieError> {
        let mut records = self.normalized_vector_records_for_index(index)?;
        records.retain(|record| !deltas.iter().any(|(id, _)| id == &record.id));
        records.extend(deltas.iter().filter_map(|(_, record)| record.clone()));
        records.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(records)
    }

    /// Brings every vector index of a changed collection up to date with the
    /// batch's document writes, inside the writing transaction.
    pub(in crate::midge::adapter) fn refresh_vector_index_states_in_tx(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        row_schema: &super::super::RowSchema,
        indexes: &[VectorIndexRecord],
        writes: &[VectorFieldWrites],
        (target_generation, outcome): (u64, &mut VectorMaintenanceOutcome),
    ) -> Result<(), CassieError> {
        for index in indexes {
            let field_id = vector_field_id(row_schema, &index.field)?;
            let deltas = writes
                .iter()
                .find(|candidate| candidate.field == index.field)
                .map_or(&[][..], |candidate| candidate.deltas.as_slice());
            match index.metadata.index_type {
                VectorIndexType::Hnsw => self.maintain_hnsw_in_tx(
                    tx,
                    (row_schema.relation_id, field_id),
                    index,
                    deltas,
                    (target_generation, outcome),
                )?,
                VectorIndexType::IvfFlat => {
                    let records = self.vector_population_after(index, deltas)?;
                    let state = VectorIndexState {
                        built_generation: 0,
                        hnsw_graph: None,
                        ivfflat_training: Some(Self::build_ivfflat_training_from_records(
                            index, &records,
                        )),
                    };
                    Self::write_vector_index_state_to_tx(
                        tx,
                        row_schema.relation_id,
                        field_id,
                        &state,
                    )?;
                    if let Some(training) = state.ivfflat_training.as_ref() {
                        Self::write_vector_source_summary_to_tx(
                            tx,
                            row_schema.relation_id,
                            field_id,
                            0,
                            training.source_fingerprint,
                            training.row_count,
                        )?;
                    }
                }
                VectorIndexType::BruteForce => {}
            }
        }
        Ok(())
    }

    fn maintain_hnsw_in_tx(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        ids: (u64, u32),
        index: &VectorIndexRecord,
        deltas: &Deltas,
        (target_generation, outcome): (u64, &mut VectorMaintenanceOutcome),
    ) -> Result<(), CassieError> {
        let rows_hint = match Self::apply_hnsw_deltas(tx, ids, index, deltas, target_generation)? {
            Incremental::Applied(tally) => {
                outcome.tallies.push(tally);
                return Ok(());
            }
            Incremental::Rebuild { rows_hint } => rows_hint,
        };
        let options = index.metadata.hnsw.clone().unwrap_or_default();
        let node_bytes = index
            .metadata
            .dimensions
            .saturating_mul(4)
            .saturating_add(options.m.saturating_mul(32))
            .saturating_add(256);
        let too_large = |rows: usize| rows.saturating_mul(node_bytes) > IN_COMMIT_REBUILD_MAX_BYTES;
        let records = if rows_hint.is_some_and(|rows| too_large(rows.saturating_add(deltas.len())))
        {
            None
        } else {
            Some(self.vector_population_after(index, deltas)?)
                .filter(|records| !too_large(records.len()))
        };
        let Some(records) = records else {
            Self::unpublish_hnsw_in_tx(
                tx,
                ids,
                (&index.collection, &index.field),
                target_generation,
            )?;
            outcome.deferred.push(DeferredVectorRebuild {
                collection: index.collection.clone(),
                field: index.field.clone(),
                generation: target_generation,
            });
            return Ok(());
        };
        let graph = Self::build_hnsw_graph_from_records(index, records);
        let tally = VectorMaintenanceTally {
            documents: deltas.len(),
            nodes_written: graph.nodes.len(),
            full_rebuild: true,
            ..VectorMaintenanceTally::default()
        };
        let state = VectorIndexState {
            built_generation: 0,
            hnsw_graph: Some(graph),
            ivfflat_training: None,
        };
        Self::write_vector_index_state_to_tx(tx, ids.0, ids.1, &state)?;
        if let Some(graph) = state.hnsw_graph.as_ref() {
            Self::write_hnsw_source_summary_to_tx(tx, ids.0, ids.1, 0, graph)?;
        }
        outcome.tallies.push(tally);
        Ok(())
    }

    /// Deletes the manifest and source summary so readers take the exact
    /// fallback, and records the durable debt that a rebuild repays.
    fn unpublish_hnsw_in_tx(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        (relation_id, field_id): (u64, u32),
        (collection, field): (&str, &str),
        target_generation: u64,
    ) -> Result<(), CassieError> {
        tx.delete(Self::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?;
        tx.delete(super::super::key_encoding::hnsw_source_summary_key(
            relation_id,
            field_id,
        ))
        .map_err(CassieError::from)?;
        Self::record_vector_maintenance_debt_in_tx(tx, collection, field, target_generation)
    }

    /// Applies `deltas` to the persisted graph node by node, or reports that
    /// the graph must be rebuilt instead.
    fn apply_hnsw_deltas(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        (relation_id, field_id): (u64, u32),
        index: &VectorIndexRecord,
        deltas: &Deltas,
        target_generation: u64,
    ) -> Result<Incremental, CassieError> {
        let Some(raw) = tx
            .get(&Self::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?
        else {
            return Ok(Incremental::Rebuild { rows_hint: None });
        };
        let Some(manifest) = decode_vector_index_state(&raw)
            .ok()
            .filter(|persisted| {
                persisted.built_generation.checked_add(1) == Some(target_generation)
            })
            .and_then(|persisted| persisted.hnsw_graph)
        else {
            return Ok(Incremental::Rebuild { rows_hint: None });
        };
        let rows_hint = Some(manifest.row_count);
        let current = manifest.version == HNSW_GRAPH_VERSION
            && manifest.metric == index.metadata.metric
            && manifest.dimensions == index.metadata.dimensions;
        if !current || deltas.len() > INCREMENTAL_DOCUMENT_LIMIT {
            return Ok(Incremental::Rebuild { rows_hint });
        }
        let (maintained, documents) = {
            let reader: &crate::midge::adapter::AdapterTransaction = tx;
            let mut maintainer = GraphMaintainer::new(
                MaintainerSeed {
                    entry_point: manifest.entry_point.clone(),
                    max_layer: manifest.max_layer,
                    row_count: manifest.row_count,
                    source_fingerprint: manifest.source_fingerprint,
                    metric: manifest.metric,
                    options: index.metadata.hnsw.clone().unwrap_or_default(),
                },
                |id| {
                    let key =
                        super::super::key_encoding::hnsw_graph_node_key(relation_id, field_id, id);
                    let Some(raw) = reader.get(&key).map_err(CassieError::from)? else {
                        return Ok(None);
                    };
                    decode_hnsw_node(&raw, id)
                        .map(Some)
                        .map_err(|_| MaintainError::Inconsistent("invalid-node"))
                },
            );
            match Self::apply_deltas_to_maintainer(&mut maintainer, deltas) {
                Ok(documents) => (maintainer.finish(), documents),
                Err(MaintainError::Inconsistent(reason)) => {
                    tracing::warn!(
                        reason,
                        collection = %index.collection,
                        field = %index.field,
                        "hnsw graph is inconsistent; rebuilding instead of maintaining it"
                    );
                    return Ok(Incremental::Rebuild { rows_hint });
                }
                Err(MaintainError::Store(error)) => return Err(error),
            }
        };
        let tally = VectorMaintenanceTally {
            documents,
            nodes_read: maintained.nodes_read,
            nodes_written: maintained.puts.len(),
            nodes_deleted: maintained.deletes.len(),
            full_rebuild: false,
        };
        Self::write_maintained_graph(tx, (relation_id, field_id), &manifest, maintained)?;
        Ok(Incremental::Applied(tally))
    }

    /// Stages the changed nodes and the new manifest and summary.
    fn write_maintained_graph(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        (relation_id, field_id): (u64, u32),
        manifest: &super::codec::PersistedHnswManifest,
        maintained: crate::vector::hnsw::MaintainedGraph,
    ) -> Result<(), CassieError> {
        for node in &maintained.puts {
            tx.put(
                super::super::key_encoding::hnsw_graph_node_key(relation_id, field_id, &node.id),
                encode_hnsw_node(node)?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        for id in &maintained.deletes {
            tx.delete(super::super::key_encoding::hnsw_graph_node_key(
                relation_id,
                field_id,
                id,
            ))
            .map_err(CassieError::from)?;
        }
        let header = HnswGraphState {
            version: HNSW_GRAPH_VERSION,
            source_fingerprint: maintained.source_fingerprint,
            row_count: maintained.row_count,
            dimensions: manifest.dimensions,
            metric: manifest.metric,
            entry_point: maintained.entry_point,
            max_layer: maintained.max_layer,
            nodes: Vec::new(),
        };
        let state = VectorIndexState {
            built_generation: 0,
            hnsw_graph: Some(header),
            ivfflat_training: None,
        };
        Self::write_vector_index_manifest_to_tx(tx, relation_id, field_id, &state)?;
        if let Some(graph) = state.hnsw_graph.as_ref() {
            Self::write_hnsw_source_summary_to_tx(tx, relation_id, field_id, 0, graph)?;
        }
        check_document_write_failure_point(DocumentWriteFailurePoint::VectorState)?;
        Ok(())
    }

    fn apply_deltas_to_maintainer<L>(
        maintainer: &mut GraphMaintainer<L>,
        deltas: &Deltas,
    ) -> Result<usize, MaintainError>
    where
        L: FnMut(&str) -> Result<Option<crate::embeddings::HnswGraphNode>, MaintainError>,
    {
        let mut changed = 0usize;
        for (id, record) in deltas {
            let stored = maintainer.stored_vector(id)?;
            match (stored, record) {
                (None, None) => {}
                (Some(_), None) => {
                    maintainer.remove(id)?;
                    changed += 1;
                }
                (None, Some(record)) => {
                    maintainer.insert(record)?;
                    changed += 1;
                }
                (Some((vector, magnitude)), Some(record)) => {
                    if vector != record.values || magnitude.to_bits() != record.magnitude.to_bits()
                    {
                        maintainer.remove(id)?;
                        maintainer.insert(record)?;
                        changed += 1;
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Rebuilds one HNSW index from the committed documents and republishes it
    /// in bounded transactions. Repays maintenance debt after a commit left the
    /// index unpublished.
    pub(in crate::midge::adapter) fn rebuild_hnsw_index_for_repair(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<(), CassieError> {
        let Some(index) = self.get_vector_index_definition(collection, field)? else {
            return Ok(());
        };
        if index.metadata.index_type != VectorIndexType::Hnsw {
            return Ok(());
        }
        let records = self.normalized_vector_records_for_index(&index)?;
        let state = VectorIndexState {
            built_generation: 0,
            hnsw_graph: Some(Self::build_hnsw_graph_from_records(&index, records.clone())),
            ivfflat_training: None,
        };
        self.write_initial_vector_index_sidecars(&index, &records, &state)
    }

    /// Records a durable commit's maintenance costs, then repairs the indexes
    /// the commit unpublished because they were too large to rebuild inside it.
    pub(in crate::midge::adapter) fn publish_vector_maintenance(
        &self,
        outcome: &VectorMaintenanceOutcome,
    ) {
        for tally in &outcome.tallies {
            self.vector_index_maintenance_metrics.record_commit(tally);
        }
        for deferred in &outcome.deferred {
            self.vector_index_maintenance_metrics.record_deferred();
            self.repair_vector_maintenance_debt(
                &deferred.collection,
                &deferred.field,
                deferred.generation,
            );
        }
    }

    /// Stamps the batch's changed normalized vectors with the new generation.
    /// Unchanged records keep the generation they were last written at.
    pub(in crate::midge::adapter) fn stamp_normalized_vectors_generation_in_tx(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        row_schema: &super::super::RowSchema,
        built_generation: u64,
        writes: &[VectorFieldWrites],
    ) -> Result<(), CassieError> {
        for write in writes {
            let field_id = vector_field_id(row_schema, &write.field)?;
            for record in write
                .deltas
                .iter()
                .filter_map(|(_, record)| record.as_ref())
            {
                let mut record = record.clone();
                record.built_generation = built_generation;
                tx.put(
                    Self::normalized_vector_key(row_schema.relation_id, field_id, &record.id),
                    super::codec::encode_normalized_vector(&record)?,
                    None,
                )
                .map_err(CassieError::from)?;
            }
        }
        Ok(())
    }
}
