use serde::{Deserialize, Serialize};

use super::{check_document_write_failure_point, DocumentWriteFailurePoint};

use super::{encode_row, CassieError, Midge, Uuid, WriteOptions};
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
        let generation = Self::increment_collection_generation_in_tx(&mut tx, collection)?;
        if let Some(graph) = graph.as_ref() {
            Self::write_graph_manifest_in_tx(
                &mut tx,
                graph.storage_id,
                generation,
                u64::try_from(ids.len()).unwrap_or(u64::MAX),
            )?;
        }
        Self::record_column_batch_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::record_projection_hash_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::increment_data_epoch_in_tx(&mut tx)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        let _ = self.complete_column_batch_maintenance(collection, generation);
        let _ = self.complete_projection_hash_maintenance(collection, generation, row_delta);
        Ok(ids)
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
