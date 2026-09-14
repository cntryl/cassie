use serde::{Deserialize, Serialize};

use super::{check_document_write_failure_point, DocumentWriteFailurePoint};

use super::{encode_row, CassieError, Midge, Uuid};
use crate::catalog::name_matches;

#[path = "graphs/reconcile.rs"]
mod reconcile;
#[path = "graphs/scan.rs"]
mod scan;

pub(crate) const GRAPH_ADJACENCY_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GraphAdjacencyManifest {
    pub(crate) format_version: u32,
    pub(crate) source_generation: u64,
    pub(crate) edge_count: u64,
}

pub(crate) enum GraphEdgeScanOutcome {
    Native {
        edges: Vec<GraphEdgeRecord>,
        memory: crate::runtime::QueryMemoryReservation,
        reads: usize,
    },
    Fallback(&'static str),
}

pub(crate) struct GraphEdgeScanRequest<'a> {
    pub(crate) graph: &'a crate::catalog::GraphMeta,
    pub(crate) node_type: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) direction: &'a str,
    pub(crate) edge_types: &'a [String],
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GraphEdgeRecord {
    pub graph: String,
    pub graph_id: u64,
    pub edge_id: String,
    pub source_type: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub edge_type: String,
    pub weight: f64,
}

impl Midge {
    /// Load documents for a newly-created graph fixture collection.
    ///
    /// This intentionally skips replacement checks and secondary-index maintenance; callers must
    /// only use it for fresh row-store graph node/edge collections.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_fresh_graph_documents(
        &self,
        collection: &str,
        documents: Vec<(Option<String>, serde_json::Value)>,
    ) -> Result<Vec<String>, CassieError> {
        let canonical_collection = self.canonical_collection_name(collection);
        let collection = canonical_collection.as_str();
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        if self.collection_uses_column_store(collection)? {
            return Err(CassieError::Unsupported(
                "fresh graph document load requires row storage".to_string(),
            ));
        }
        if self
            .list_indexes()?
            .iter()
            .any(|index| index.collection.eq_ignore_ascii_case(collection))
            || self
                .list_vector_indexes_canonical()?
                .iter()
                .any(|index| index.collection.eq_ignore_ascii_case(collection))
        {
            return Err(CassieError::Unsupported(
                "fresh graph document load does not maintain secondary indexes".to_string(),
            ));
        }

        let schema = self
            .collection_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let row_schema = self.row_schema(collection)?;
        let graph = self.graph_for_edge_collection(collection)?;
        let write_gate = self.collection_write_gate(collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        let generation = Self::increment_collection_generation_in_tx(&mut tx, collection)?;
        let prior_edge_count = graph
            .as_ref()
            .map(|graph| Self::fresh_graph_edge_count_in_tx(&tx, graph, generation.wrapping_sub(1)))
            .transpose()?;
        let mut ids = Vec::with_capacity(documents.len());

        for (id, payload) in documents {
            Self::validate_document(&schema, &payload)?;
            let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let row_blob = encode_row(&row_schema, &payload)?;
            tx.put(Self::row_key(row_schema.relation_id, &id), row_blob, None)
                .map_err(CassieError::from)?;
            Self::write_document_hash_to_tx(&mut tx, collection, &id, &row_schema, &payload)?;

            if let Some(graph) = graph.as_ref() {
                let record = graph_edge_record_from_payload(graph, &id, &payload, true)?
                    .ok_or_else(|| {
                        CassieError::Unsupported("graph edge payload is incomplete".into())
                    })?;
                Self::put_graph_edge_record(&mut tx, &record)?;
            }
            ids.push(id);
        }

        let row_delta = i64::try_from(ids.len()).unwrap_or(i64::MAX);
        if let Some(graph) = graph.as_ref() {
            let batch_edge_count = u64::try_from(ids.len()).map_err(|_| {
                CassieError::ResourceLimit("fresh graph edge count overflow".to_string())
            })?;
            let prior_edge_count = prior_edge_count.ok_or_else(|| {
                CassieError::Execution(
                    "fresh graph edge batch is missing its prior manifest count".to_string(),
                )
            })?;
            let edge_count = prior_edge_count
                .checked_add(batch_edge_count)
                .ok_or_else(|| {
                    CassieError::ResourceLimit("fresh graph edge count overflow".to_string())
                })?;
            Self::write_graph_manifest_in_tx(&mut tx, graph.storage_id, generation, edge_count)?;
        }
        Self::record_column_batch_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::record_projection_hash_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::increment_data_epoch_in_tx(&mut tx)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        let _ = self.complete_column_batch_maintenance(collection, generation, None);
        let _ = self.complete_projection_hash_maintenance(collection, generation, row_delta);
        Ok(ids)
    }

    fn fresh_graph_edge_count_in_tx(
        tx: &cntryl_midge::Transaction,
        graph: &crate::catalog::GraphMeta,
        expected_generation: u64,
    ) -> Result<u64, CassieError> {
        let key = super::key_encoding::graph_manifest_key(graph.storage_id);
        let raw = tx.get(&key).map_err(CassieError::from)?.ok_or_else(|| {
            CassieError::Parse(format!(
                "graph '{}' has no adjacency manifest before fresh batch",
                graph.name
            ))
        })?;
        let manifest = serde_json::from_slice::<GraphAdjacencyManifest>(&raw).map_err(|error| {
            CassieError::Parse(format!(
                "graph '{}' has an invalid adjacency manifest before fresh batch: {error}",
                graph.name
            ))
        })?;
        if manifest.format_version != GRAPH_ADJACENCY_FORMAT_VERSION
            || manifest.source_generation != expected_generation
        {
            return Err(CassieError::Parse(format!(
                "graph '{}' adjacency manifest is stale before fresh batch",
                graph.name
            )));
        }
        Ok(manifest.edge_count)
    }

    pub(crate) fn graph_for_edge_collection(
        &self,
        collection: &str,
    ) -> Result<Option<crate::catalog::GraphMeta>, CassieError> {
        Ok(self.list_graphs()?.into_iter().find(|graph| {
            name_matches(&graph.edge_collection, collection)
                || name_matches(collection, &graph.edge_collection)
        }))
    }

    pub(crate) fn sync_graph_adjacency_for_document(
        tx: &mut cntryl_midge::Transaction,
        graph: Option<&crate::catalog::GraphMeta>,
        row_id: &str,
        previous: Option<&serde_json::Value>,
        next: Option<&serde_json::Value>,
        target_generation: u64,
    ) -> Result<(usize, usize), CassieError> {
        let Some(graph) = graph else {
            return Ok((0, 0));
        };

        let previous_record = previous
            .map(|payload| graph_edge_record_from_payload(graph, row_id, payload, false))
            .transpose()?
            .flatten();
        let next_record = next
            .map(|payload| graph_edge_record_from_payload(graph, row_id, payload, true))
            .transpose()?
            .flatten();

        let mut deletes = 0usize;
        if let Some(record) = previous_record.as_ref() {
            Self::delete_graph_edge_record(tx, record)?;
            deletes = deletes.saturating_add(4);
        }

        let mut puts = 0usize;
        if next.is_some() && next_record.is_none() {
            return Err(CassieError::Unsupported(
                "graph edge payload is incomplete".into(),
            ));
        }
        if let Some(record) = next_record.as_ref() {
            Self::put_graph_edge_record(tx, record)?;
            puts = puts.saturating_add(4);
        }

        Self::advance_graph_manifest_in_tx(
            tx,
            graph.storage_id,
            target_generation,
            previous_record.is_some(),
            next_record.is_some(),
        )?;

        check_document_write_failure_point(DocumentWriteFailurePoint::GraphAdjacency)?;

        Ok((deletes, puts))
    }

    fn put_graph_edge_record(
        tx: &mut cntryl_midge::Transaction,
        record: &GraphEdgeRecord,
    ) -> Result<(), CassieError> {
        tx.put(Self::graph_outbound_edge_key(record), Vec::new(), None)
            .map_err(CassieError::from)?;
        tx.put(Self::graph_inbound_edge_key(record), Vec::new(), None)
            .map_err(CassieError::from)?;
        tx.put(
            super::key_encoding::graph_outbound_edge_type_key(record),
            Vec::new(),
            None,
        )
        .map_err(CassieError::from)?;
        tx.put(
            super::key_encoding::graph_inbound_edge_type_key(record),
            Vec::new(),
            None,
        )
        .map_err(CassieError::from)?;
        Ok(())
    }

    fn delete_graph_edge_record(
        tx: &mut cntryl_midge::Transaction,
        record: &GraphEdgeRecord,
    ) -> Result<(), CassieError> {
        tx.delete(Self::graph_outbound_edge_key(record))
            .map_err(CassieError::from)?;
        tx.delete(Self::graph_inbound_edge_key(record))
            .map_err(CassieError::from)?;
        tx.delete(super::key_encoding::graph_outbound_edge_type_key(record))
            .map_err(CassieError::from)?;
        tx.delete(super::key_encoding::graph_inbound_edge_type_key(record))
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn write_graph_manifest_in_tx(
        tx: &mut cntryl_midge::Transaction,
        graph_id: u64,
        source_generation: u64,
        edge_count: u64,
    ) -> Result<(), CassieError> {
        let manifest = GraphAdjacencyManifest {
            format_version: GRAPH_ADJACENCY_FORMAT_VERSION,
            source_generation,
            edge_count,
        };
        let raw =
            serde_json::to_vec(&manifest).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(super::key_encoding::graph_manifest_key(graph_id), raw, None)
            .map_err(CassieError::from)
    }

    fn advance_graph_manifest_in_tx(
        tx: &mut cntryl_midge::Transaction,
        graph_id: u64,
        target_generation: u64,
        had_previous: bool,
        has_next: bool,
    ) -> Result<(), CassieError> {
        let key = super::key_encoding::graph_manifest_key(graph_id);
        let Some(raw) = tx.get(&key).map_err(CassieError::from)? else {
            return Ok(());
        };
        let Ok(mut manifest) = serde_json::from_slice::<GraphAdjacencyManifest>(&raw) else {
            tx.delete(key).map_err(CassieError::from)?;
            return Ok(());
        };
        if manifest.format_version != GRAPH_ADJACENCY_FORMAT_VERSION
            || manifest.source_generation != target_generation
                && manifest.source_generation.wrapping_add(1) != target_generation
        {
            tx.delete(key).map_err(CassieError::from)?;
            return Ok(());
        }
        manifest.edge_count = match (had_previous, has_next) {
            (false, true) => manifest.edge_count.saturating_add(1),
            (true, false) if manifest.edge_count > 0 => manifest.edge_count - 1,
            (true, false) => {
                tx.delete(key).map_err(CassieError::from)?;
                return Ok(());
            }
            _ => manifest.edge_count,
        };
        Self::write_graph_manifest_in_tx(tx, graph_id, target_generation, manifest.edge_count)
    }
}

pub(crate) fn graph_edge_record_from_payload(
    graph: &crate::catalog::GraphMeta,
    row_id: &str,
    payload: &serde_json::Value,
    strict: bool,
) -> Result<Option<GraphEdgeRecord>, CassieError> {
    let edge_id = graph_text(payload, &graph.edge_id_field).unwrap_or_else(|| row_id.to_string());
    let Some(source_type) = graph_text(payload, &graph.source_type_field) else {
        return Ok(None);
    };
    let Some(source_id) = graph_text(payload, &graph.source_id_field) else {
        return Ok(None);
    };
    let Some(target_type) = graph_text(payload, &graph.target_type_field) else {
        return Ok(None);
    };
    let Some(target_id) = graph_text(payload, &graph.target_id_field) else {
        return Ok(None);
    };
    let Some(edge_type) = graph_text(payload, &graph.edge_type_field) else {
        return Ok(None);
    };
    let weight = graph_weight(payload, &graph.weight_field)?;
    if strict && weight < 0.0 {
        return Err(CassieError::Unsupported(
            "graph edge weight must be non-negative".to_string(),
        ));
    }
    Ok(Some(GraphEdgeRecord {
        graph: graph.name.clone(),
        graph_id: graph.storage_id,
        edge_id,
        source_type,
        source_id,
        target_type,
        target_id,
        edge_type,
        weight,
    }))
}

fn graph_text(payload: &serde_json::Value, field: &str) -> Option<String> {
    let value = payload.get(field)?;
    match value {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn graph_weight(payload: &serde_json::Value, field: &str) -> Result<f64, CassieError> {
    let Some(value) = payload.get(field) else {
        return Ok(1.0);
    };
    let weight = match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
    .ok_or_else(|| CassieError::Unsupported("graph edge weight must be numeric".to_string()))?;
    if !weight.is_finite() {
        return Err(CassieError::Unsupported(
            "graph edge weight must be finite".to_string(),
        ));
    }
    Ok(weight)
}

#[cfg(test)]
mod tests {
    use super::{GraphAdjacencyManifest, Midge, GRAPH_ADJACENCY_FORMAT_VERSION};
    use crate::catalog::GraphMeta;
    use crate::types::{FieldSchema, Schema};
    use serde_json::json;

    #[test]
    fn should_accumulate_graph_manifest_across_fresh_load_batches() {
        // Arrange
        let path = std::env::temp_dir().join(format!(
            "cassie_fresh_graph_batches_{}",
            uuid::Uuid::new_v4()
        ));
        let midge = Midge::new_with_data_dir(&path).expect("create Midge");
        let graph = GraphMeta::new("batched_graph");
        let edge_schema = Schema {
            fields: graph
                .edge_builtin_fields()
                .into_iter()
                .map(|(name, data_type)| FieldSchema {
                    name,
                    data_type,
                    nullable: true,
                })
                .collect(),
        };
        midge
            .create_collection(&graph.edge_collection, edge_schema)
            .expect("create graph edge collection");
        midge.put_graph(&graph).expect("persist graph metadata");
        let edge = |index: usize| {
            (
                Some(format!("edge-{index}")),
                json!({
                    "edge_id": format!("edge-{index}"),
                    "source_type": "doc",
                    "source_id": format!("node-{index}"),
                    "target_type": "doc",
                    "target_id": format!("node-{}", index + 1),
                    "edge_type": "links",
                    "weight": 1,
                }),
            )
        };

        // Act
        midge
            .put_fresh_graph_documents(&graph.edge_collection, vec![edge(0), edge(1)])
            .expect("write first fresh graph batch");
        midge
            .put_fresh_graph_documents(&graph.edge_collection, vec![edge(2)])
            .expect("write second fresh graph batch");
        let persisted_graph = midge
            .list_graphs()
            .expect("list graphs")
            .into_iter()
            .find(|candidate| candidate.name == graph.name)
            .expect("persisted graph");
        let collection = midge.canonical_collection_name(&graph.edge_collection);
        let tx = midge
            .begin_data_readonly_tx_for(&collection)
            .expect("read graph data family");
        let raw = tx
            .get(&super::super::key_encoding::graph_manifest_key(
                persisted_graph.storage_id,
            ))
            .expect("read graph manifest")
            .expect("graph manifest must exist");
        let manifest =
            serde_json::from_slice::<GraphAdjacencyManifest>(&raw).expect("decode graph manifest");

        // Assert
        assert_eq!(
            manifest,
            GraphAdjacencyManifest {
                format_version: GRAPH_ADJACENCY_FORMAT_VERSION,
                source_generation: 2,
                edge_count: 3,
            }
        );
        drop(tx);
        drop(midge);
        std::fs::remove_dir_all(path).expect("clean up fresh graph batch fixture");
    }
}
