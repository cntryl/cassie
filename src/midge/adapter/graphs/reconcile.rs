use std::collections::BTreeSet;

use super::{
    graph_edge_record_from_payload, CassieError, GraphAdjacencyManifest, GraphEdgeRecord, Midge,
    WriteOptions, GRAPH_ADJACENCY_FORMAT_VERSION,
};

impl Midge {
    /// Audit every persisted graph sidecar and rebuild any non-current artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when source rows cannot be decoded or storage recovery fails.
    pub fn reconcile_graph_adjacency(&self) -> Result<(), CassieError> {
        for graph in self.list_graphs()? {
            self.reconcile_graph_adjacency_for(&graph)?;
        }
        Ok(())
    }

    pub(crate) fn reconcile_graph_adjacency_for(
        &self,
        graph: &crate::catalog::GraphMeta,
    ) -> Result<(), CassieError> {
        let edge_collection = self.canonical_collection_name(&graph.edge_collection);
        let write_gate = self.collection_write_gate(&edge_collection);
        let _write_guard = write_gate.lock();
        if self.collection_schema(&edge_collection).is_none() {
            return self.remove_graph_adjacency(graph, &edge_collection);
        }
        let generation = self.collection_generation(&edge_collection)?;
        let records = self.load_graph_source_records(graph, &edge_collection)?;
        let entries = self.raw_scan_prefix_for_collection(
            &edge_collection,
            &super::super::key_encoding::graph_adjacency_prefix(graph.storage_id),
        )?;
        if graph_sidecar_is_current(graph, generation, &records, &entries) {
            return Ok(());
        }
        self.rebuild_graph_adjacency(graph, &edge_collection, generation, &records, entries)
    }

    fn remove_graph_adjacency(
        &self,
        graph: &crate::catalog::GraphMeta,
        edge_collection: &str,
    ) -> Result<(), CassieError> {
        let entries = self.raw_scan_prefix_for_collection(
            edge_collection,
            &super::super::key_encoding::graph_adjacency_prefix(graph.storage_id),
        )?;
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin_data_rw_tx_for(edge_collection)?;
        for (key, _) in entries {
            tx.delete(key).map_err(CassieError::from)?;
        }
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    fn load_graph_source_records(
        &self,
        graph: &crate::catalog::GraphMeta,
        edge_collection: &str,
    ) -> Result<Vec<GraphEdgeRecord>, CassieError> {
        self.scan_documents(edge_collection)?
            .into_iter()
            .map(|document| {
                graph_edge_record_from_payload(graph, &document.id, &document.payload, true)?
                    .ok_or_else(|| {
                        CassieError::Parse(format!(
                            "graph '{}' edge '{}' is incomplete",
                            graph.name, document.id
                        ))
                    })
            })
            .collect()
    }

    fn rebuild_graph_adjacency(
        &self,
        graph: &crate::catalog::GraphMeta,
        edge_collection: &str,
        generation: u64,
        records: &[GraphEdgeRecord],
        entries: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(edge_collection)?;
        for (key, _) in entries {
            tx.delete(key).map_err(CassieError::from)?;
        }
        for record in records {
            Self::put_graph_edge_record(&mut tx, record)?;
        }
        Self::write_graph_manifest_in_tx(
            &mut tx,
            graph.storage_id,
            generation,
            u64::try_from(records.len()).unwrap_or(u64::MAX),
        )?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }
}

fn graph_sidecar_is_current(
    graph: &crate::catalog::GraphMeta,
    generation: u64,
    records: &[GraphEdgeRecord],
    entries: &[(Vec<u8>, Vec<u8>)],
) -> bool {
    let manifest_key = super::super::key_encoding::graph_manifest_key(graph.storage_id);
    let Some((_, manifest_raw)) = entries.iter().find(|(key, _)| key == &manifest_key) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<GraphAdjacencyManifest>(manifest_raw) else {
        return false;
    };
    if manifest.format_version != GRAPH_ADJACENCY_FORMAT_VERSION
        || manifest.source_generation != generation
        || manifest.edge_count != u64::try_from(records.len()).unwrap_or(u64::MAX)
    {
        return false;
    }

    let actual_keys = entries
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<BTreeSet<_>>();
    if entries
        .iter()
        .any(|(key, value)| key != &manifest_key && !value.is_empty())
    {
        return false;
    }
    expected_graph_keys(graph, records) == actual_keys
}

fn expected_graph_keys(
    graph: &crate::catalog::GraphMeta,
    records: &[GraphEdgeRecord],
) -> BTreeSet<Vec<u8>> {
    let mut keys = BTreeSet::from([super::super::key_encoding::graph_manifest_key(
        graph.storage_id,
    )]);
    for record in records {
        keys.insert(super::super::key_encoding::graph_outbound_edge_key(record));
        keys.insert(super::super::key_encoding::graph_inbound_edge_key(record));
        keys.insert(super::super::key_encoding::graph_outbound_edge_type_key(
            record,
        ));
        keys.insert(super::super::key_encoding::graph_inbound_edge_type_key(
            record,
        ));
    }
    keys
}
