use serde::{Deserialize, Serialize};

use super::{check_document_write_failure_point, DocumentWriteFailurePoint};

use super::{encode_row, CassieError, Midge, StorageFamily, Uuid, WriteOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GraphEdgeRecord {
    pub graph: String,
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
                .list_vector_indexes()?
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
        let mut tx = self.begin_data_rw_tx()?;
        let mut ids = Vec::with_capacity(documents.len());

        for (id, payload) in documents {
            Self::validate_document(&schema, &payload)?;
            let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let row_blob = encode_row(&row_schema, &payload)?;
            tx.put(Self::row_key(collection, &id), row_blob, None)
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
        Ok(self
            .list_graphs()?
            .into_iter()
            .find(|graph| graph.edge_collection.eq_ignore_ascii_case(collection)))
    }

    pub(crate) fn sync_graph_adjacency_for_document(
        tx: &mut cntryl_midge::Transaction,
        graph: Option<&crate::catalog::GraphMeta>,
        row_id: &str,
        previous: Option<&serde_json::Value>,
        next: Option<&serde_json::Value>,
    ) -> Result<(usize, usize), CassieError> {
        let Some(graph) = graph else {
            return Ok((0, 0));
        };

        let mut deletes = 0usize;
        if let Some(previous) = previous {
            if let Some(record) = graph_edge_record_from_payload(graph, row_id, previous, false)? {
                Self::delete_graph_edge_record(tx, &record)?;
                deletes = deletes.saturating_add(2);
            }
        }

        let mut puts = 0usize;
        if let Some(next) = next {
            let record =
                graph_edge_record_from_payload(graph, row_id, next, true)?.ok_or_else(|| {
                    CassieError::Unsupported("graph edge payload is incomplete".into())
                })?;
            Self::put_graph_edge_record(tx, &record)?;
            puts = puts.saturating_add(2);
        }

        check_document_write_failure_point(DocumentWriteFailurePoint::GraphAdjacency)?;

        Ok((deletes, puts))
    }

    pub(crate) fn scan_graph_edges(
        &self,
        graph: &crate::catalog::GraphMeta,
        node_type: &str,
        node_id: &str,
        direction: &str,
        edge_types: &[String],
    ) -> Result<Vec<GraphEdgeRecord>, CassieError> {
        let mut out = Vec::new();
        if direction.eq_ignore_ascii_case("out") || direction.eq_ignore_ascii_case("both") {
            out.extend(self.scan_graph_edges_by_prefix(
                &Self::graph_outbound_prefix(&graph.name, node_type, node_id),
                edge_types,
            )?);
        }
        if direction.eq_ignore_ascii_case("in") || direction.eq_ignore_ascii_case("both") {
            out.extend(self.scan_graph_edges_by_prefix(
                &Self::graph_inbound_prefix(&graph.name, node_type, node_id),
                edge_types,
            )?);
        }
        out.sort_by(|left, right| {
            left.weight
                .total_cmp(&right.weight)
                .then_with(|| left.edge_id.cmp(&right.edge_id))
        });
        Ok(out)
    }

    fn scan_graph_edges_by_prefix(
        &self,
        prefix: &[u8],
        edge_types: &[String],
    ) -> Result<Vec<GraphEdgeRecord>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Data, prefix)?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let record: GraphEdgeRecord = serde_json::from_slice(&raw_value)
                .map_err(|error| CassieError::Parse(format!("invalid graph edge: {error}")))?;
            if edge_types.is_empty()
                || edge_types
                    .iter()
                    .any(|edge_type| edge_type.eq_ignore_ascii_case(&record.edge_type))
            {
                out.push(record);
            }
        }
        Ok(out)
    }

    fn put_graph_edge_record(
        tx: &mut cntryl_midge::Transaction,
        record: &GraphEdgeRecord,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            Self::graph_outbound_edge_key(
                &record.graph,
                &record.source_type,
                &record.source_id,
                &record.edge_type,
                &record.target_type,
                &record.target_id,
                &record.edge_id,
            ),
            value.clone(),
            None,
        )
        .map_err(CassieError::from)?;
        tx.put(
            Self::graph_inbound_edge_key(
                &record.graph,
                &record.target_type,
                &record.target_id,
                &record.edge_type,
                &record.source_type,
                &record.source_id,
                &record.edge_id,
            ),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        Ok(())
    }

    fn delete_graph_edge_record(
        tx: &mut cntryl_midge::Transaction,
        record: &GraphEdgeRecord,
    ) -> Result<(), CassieError> {
        tx.delete(Self::graph_outbound_edge_key(
            &record.graph,
            &record.source_type,
            &record.source_id,
            &record.edge_type,
            &record.target_type,
            &record.target_id,
            &record.edge_id,
        ))
        .map_err(CassieError::from)?;
        tx.delete(Self::graph_inbound_edge_key(
            &record.graph,
            &record.target_type,
            &record.target_id,
            &record.edge_type,
            &record.source_type,
            &record.source_id,
            &record.edge_id,
        ))
        .map_err(CassieError::from)?;
        Ok(())
    }
}

fn graph_edge_record_from_payload(
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
