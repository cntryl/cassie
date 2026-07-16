use serde::{Deserialize, Serialize};

use super::{check_document_write_failure_point, DocumentWriteFailurePoint};

use super::{encode_row, CassieError, Midge, Uuid, WriteOptions};
use crate::catalog::name_matches;

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

struct GraphEdgeScan<'a> {
    graph: &'a crate::catalog::GraphMeta,
    database: &'a str,
    prefix: &'a [u8],
    direction: &'a str,
    node_type: &'a str,
    node_id: &'a str,
    edge_types: &'a [String],
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
        let mut graph = graph.clone();
        if graph.storage_id == 0 {
            graph.storage_id = self
                .list_graphs()?
                .into_iter()
                .find(|stored| crate::catalog::name_matches(&stored.name, &graph.name))
                .ok_or_else(|| CassieError::Parse(format!("graph '{}' not found", graph.name)))?
                .storage_id;
        }
        let mut out = Vec::new();
        if direction.eq_ignore_ascii_case("out") || direction.eq_ignore_ascii_case("both") {
            out.extend(
                self.scan_graph_edges_by_prefix(&GraphEdgeScan {
                    graph: &graph,
                    database: crate::catalog::relation_database_name(&graph.name)
                        .as_deref()
                        .unwrap_or(self.default_database.as_str()),
                    prefix: &Self::graph_outbound_prefix(graph.storage_id, node_type, node_id),
                    direction: "out",
                    node_type,
                    node_id,
                    edge_types,
                })?,
            );
        }
        if direction.eq_ignore_ascii_case("in") || direction.eq_ignore_ascii_case("both") {
            out.extend(
                self.scan_graph_edges_by_prefix(&GraphEdgeScan {
                    graph: &graph,
                    database: crate::catalog::relation_database_name(&graph.name)
                        .as_deref()
                        .unwrap_or(self.default_database.as_str()),
                    prefix: &Self::graph_inbound_prefix(graph.storage_id, node_type, node_id),
                    direction: "in",
                    node_type,
                    node_id,
                    edge_types,
                })?,
            );
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
        request: &GraphEdgeScan<'_>,
    ) -> Result<Vec<GraphEdgeRecord>, CassieError> {
        let entries = self.raw_scan_prefix_database(request.database, request.prefix)?;
        let mut out = Vec::with_capacity(entries.len());
        for (key, raw_value) in entries {
            if !raw_value.is_empty() {
                return Err(CassieError::Parse(
                    "graph adjacency values must be empty".to_string(),
                ));
            }
            let record = decode_graph_edge_key(
                request.graph,
                request.prefix,
                &key,
                request.direction,
                request.node_type,
                request.node_id,
            )?;
            if request.edge_types.is_empty()
                || request
                    .edge_types
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
        tx.put(Self::graph_outbound_edge_key(record), Vec::new(), None)
            .map_err(CassieError::from)?;
        tx.put(Self::graph_inbound_edge_key(record), Vec::new(), None)
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
        Ok(())
    }
}

fn decode_graph_edge_key(
    graph: &crate::catalog::GraphMeta,
    prefix: &[u8],
    key: &[u8],
    direction: &str,
    node_type: &str,
    node_id: &str,
) -> Result<GraphEdgeRecord, CassieError> {
    let suffix = key
        .strip_prefix(prefix)
        .ok_or_else(|| CassieError::Parse("invalid graph adjacency prefix".to_string()))?;
    let components = suffix
        .split(|byte| *byte == 0)
        .filter(|component| !component.is_empty())
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CassieError::Parse(format!("invalid graph adjacency key: {error}")))?;
    let [edge_type, weight, other_type, other_id, edge_id] = components.as_slice() else {
        return Err(CassieError::Parse(
            "invalid graph adjacency component count".to_string(),
        ));
    };
    let weight = decode_sortable_weight(weight)?;
    let (source_type, source_id, target_type, target_id) = if direction == "out" {
        (node_type, node_id, *other_type, *other_id)
    } else {
        (*other_type, *other_id, node_type, node_id)
    };
    Ok(GraphEdgeRecord {
        graph: graph.name.clone(),
        graph_id: graph.storage_id,
        edge_id: (*edge_id).to_string(),
        source_type: source_type.to_string(),
        source_id: source_id.to_string(),
        target_type: target_type.to_string(),
        target_id: target_id.to_string(),
        edge_type: (*edge_type).to_string(),
        weight,
    })
}

fn decode_sortable_weight(value: &str) -> Result<f64, CassieError> {
    let ordered = u64::from_str_radix(value, 16)
        .map_err(|error| CassieError::Parse(format!("invalid graph weight: {error}")))?;
    let bits = if ordered & (1_u64 << 63) == 0 {
        !ordered
    } else {
        ordered ^ (1_u64 << 63)
    };
    Ok(f64::from_bits(bits))
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
