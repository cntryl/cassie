use std::mem::size_of;

use cntryl_midge::{Query, Transaction};

use crate::runtime::accounted::Accounted;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::super::streaming_scans::{provisional_controlled_document_bytes, AccountedDocument};
use super::super::{decode_row, key_encoding, DocumentRef};
use super::{
    graph_edge_record_from_payload, CassieError, GraphAdjacencyManifest, GraphEdgeRecord, Midge,
};

type ExpectedKey = (Accounted<Vec<u8>>, bool);

struct ExpectedKeys {
    values: Vec<ExpectedKey>,
    memory: QueryMemoryReservation,
}

impl ExpectedKeys {
    fn try_new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            values: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    fn try_push(
        &mut self,
        key: Accounted<Vec<u8>>,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .capacity()
                .checked_add((self.values.capacity() / 2).max(1))
                .ok_or_else(overflow)?;
            let bytes = capacity
                .checked_mul(size_of::<ExpectedKey>())
                .ok_or_else(overflow)?;
            // The existing slot reservation and every independent key owner stay
            // live while the complete replacement capacity is admitted.
            let mut replacement = controls.reserve_query_memory(bytes)?;
            self.values
                .try_reserve_exact(capacity - self.values.len())
                .map_err(|error| {
                    CassieError::ResourceLimit(format!(
                        "unable to grow graph verification slots: {error}"
                    ))
                })?;
            let actual_bytes = self
                .values
                .capacity()
                .checked_mul(size_of::<ExpectedKey>())
                .ok_or_else(overflow)?;
            if actual_bytes > bytes {
                return Err(overflow());
            }
            replacement.shrink_to(actual_bytes);
            self.memory = replacement;
        }
        self.values.push((key, false));
        Ok(())
    }

    #[cfg(test)]
    fn into_parts(self) -> (Vec<ExpectedKey>, QueryMemoryReservation) {
        (self.values, self.memory)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.values.len()
    }
}

impl Midge {
    pub(super) fn verify_graph_adjacency_controlled(
        &self,
        tx: &Transaction,
        graph: &crate::catalog::GraphMeta,
        manifest: &GraphAdjacencyManifest,
        controls: &QueryExecutionControls,
        reads: &mut usize,
    ) -> Result<bool, CassieError> {
        let collection = self.canonical_collection_name(&graph.edge_collection);
        let schema = self.row_schema(&collection)?;
        let prefix = Self::row_prefix(schema.relation_id);
        let mut keys = ExpectedKeys::try_new(controls)?;
        let mut edge_count = 0_u64;
        scan_entries(self, tx, &prefix, controls, reads, |key, value| {
            let bytes = provisional_controlled_document_bytes(
                &schema,
                None,
                false,
                key.len(),
                value.len(),
            )?;
            let Some(document) =
                AccountedDocument::try_build_fresh_decode_optional(controls, bytes, || {
                    let Some(id) = key_encoding::utf8_suffix_after_prefix(key, &prefix) else {
                        return Ok(None);
                    };
                    if id.is_empty() {
                        return Ok(None);
                    }
                    Ok(Some(DocumentRef {
                        id,
                        payload: decode_row(&schema, value)?,
                    }))
                })?
            else {
                return Ok(true);
            };
            let record_bytes =
                record_construction_bytes(graph, document.id(), &document.document().payload)?;
            let _record_memory = controls.reserve_query_memory(record_bytes)?;
            let Some(record) = graph_edge_record_from_payload(
                graph,
                document.id(),
                &document.document().payload,
                true,
            )?
            else {
                return Ok(true);
            };
            edge_count = edge_count.checked_add(1).ok_or_else(overflow)?;
            for constructor in [
                key_encoding::graph_outbound_edge_key,
                key_encoding::graph_inbound_edge_key,
                key_encoding::graph_outbound_edge_type_key,
                key_encoding::graph_inbound_edge_type_key,
            ] {
                check_controls(controls)?;
                let key = build_accounted_key(&record, constructor, controls)?;
                push_expected_key(&mut keys, key, controls)?;
            }
            Ok(true)
        })?;
        if edge_count != manifest.edge_count {
            return Ok(false);
        }
        check_controls(controls)?;
        keys.values
            .sort_unstable_by(|left, right| left.0.get().cmp(right.0.get()));
        check_controls(controls)?;
        if keys
            .values
            .windows(2)
            .any(|pair| pair[0].0.get() == pair[1].0.get())
        {
            return Ok(false);
        }
        let manifest_key = key_encoding::graph_manifest_key(graph.storage_id);
        let adjacency_prefix = key_encoding::graph_adjacency_prefix(graph.storage_id);
        let consistent = scan_entries(
            self,
            tx,
            &adjacency_prefix,
            controls,
            reads,
            |key, value| {
                if key == manifest_key {
                    return Ok(true);
                }
                if !value.is_empty() {
                    return Ok(false);
                }
                let Ok(index) = keys
                    .values
                    .binary_search_by(|entry| entry.0.get().as_slice().cmp(key))
                else {
                    return Ok(false);
                };
                if keys.values[index].1 {
                    return Ok(false);
                }
                keys.values[index].1 = true;
                Ok(true)
            },
        )?;
        check_controls(controls)?;
        Ok(consistent && keys.values.iter().all(|entry| entry.1))
    }
}

fn scan_entries(
    midge: &Midge,
    tx: &Transaction,
    prefix: &[u8],
    controls: &QueryExecutionControls,
    reads: &mut usize,
    mut visit: impl FnMut(&[u8], &[u8]) -> Result<bool, CassieError>,
) -> Result<bool, CassieError> {
    check_controls(controls)?;
    // Midge exposes a lazy snapshot iterator. Keep one cursor per namespace;
    // reopening a cursor after each small page repeats SST cursor setup.
    let _query_memory = controls.reserve_query_memory(prefix.len())?;
    let query = Query::new().prefix(prefix.to_vec().into());
    let mut scan = tx.scan(&query).map_err(CassieError::from)?;
    loop {
        check_controls(controls)?;
        let Some(entry) = scan.next() else {
            return Ok(true);
        };
        check_controls(controls)?;
        let (key, value) = entry.map_err(CassieError::from)?;
        midge.record_query_scan_entry();
        *reads = reads.checked_add(1).ok_or_else(overflow)?;
        if super::super::query_scan_control::should_cancel_controlled_query_scan(controls) {
            return Err(CassieError::QueryCancelled);
        }
        if !visit(&key, &value)? {
            return Ok(false);
        }
    }
}

fn record_construction_bytes(
    graph: &crate::catalog::GraphMeta,
    id: &str,
    payload: &serde_json::Value,
) -> Result<usize, CassieError> {
    let mut bytes = size_of::<GraphEdgeRecord>()
        .checked_add(graph.name.len())
        .and_then(|n| n.checked_add(id.len()))
        .ok_or_else(overflow)?;
    for field in [
        &graph.edge_id_field,
        &graph.source_type_field,
        &graph.source_id_field,
        &graph.target_type_field,
        &graph.target_id_field,
        &graph.edge_type_field,
    ] {
        let field_bytes = match payload.get(field) {
            Some(serde_json::Value::String(value)) => value.len(),
            Some(serde_json::Value::Number(_)) => 32,
            _ if field == &graph.edge_id_field => id.len(),
            _ => 0,
        };
        bytes = bytes.checked_add(field_bytes).ok_or_else(overflow)?;
    }
    Ok(bytes)
}

fn build_accounted_key(
    record: &GraphEdgeRecord,
    constructor: fn(&GraphEdgeRecord) -> Vec<u8>,
    controls: &QueryExecutionControls,
) -> Result<Accounted<Vec<u8>>, CassieError> {
    // Cover encoded bytes, component slots, lowercase temporaries and encoder
    // growth before construction; retain only the actual key capacity afterward.
    let variable = [
        &record.row_id,
        &record.edge_id,
        &record.edge_type,
        &record.source_type,
        &record.source_id,
        &record.target_type,
        &record.target_id,
    ]
    .iter()
    .try_fold(0_usize, |bytes, field| {
        bytes.checked_add(field.len()).ok_or_else(overflow)
    })?;
    let bound = variable
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(1024))
        .ok_or_else(overflow)?;
    let mut memory = controls.reserve_query_memory(bound)?;
    let key = constructor(record);
    let retained = key
        .capacity()
        .checked_add(size_of::<Vec<u8>>())
        .ok_or_else(overflow)?;
    if retained > bound {
        return Err(overflow());
    }
    memory.shrink_to(retained);
    Ok(Accounted::from_admitted(key, memory))
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("graph verification retained-size accounting overflow".to_owned())
}

fn push_expected_key(
    keys: &mut ExpectedKeys,
    key: Accounted<Vec<u8>>,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    keys.try_push(key, controls)
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::catalog::GraphMeta;
    use crate::config::CassieRuntimeLimits;

    fn controls(budget: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    #[test]
    fn should_admit_graph_key_scratch_before_calling_the_constructor() {
        fn forbidden(_: &GraphEdgeRecord) -> Vec<u8> {
            panic!("denied construction must not run");
        }
        // Arrange
        let graph = GraphMeta::new("scratch");
        let payload = serde_json::json!({"edge_id":"e", "source_type":"p", "source_id":"a", "target_type":"p", "target_id":"b", "edge_type":"k", "weight":1});
        let record = graph_edge_record_from_payload(&graph, "row", &payload, true)
            .expect("edge")
            .expect("complete edge");
        let controls = controls(1);

        // Act
        let result = build_accounted_key(&record, forbidden, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_admit_old_new_verification_slots_before_growing_the_key_vector() {
        // Arrange
        let slot = size_of::<ExpectedKey>();
        let inline = size_of::<Vec<u8>>();
        let controls = controls(2 * inline + 3 * slot - 1);
        let mut keys = ExpectedKeys::try_new(&controls).expect("empty keys");
        let first = Accounted::try_new(&controls, inline, Vec::new).expect("first key");
        push_expected_key(&mut keys, first, &controls).expect("first slot");
        let second = Accounted::try_new(&controls, inline, Vec::new).expect("second key");

        // Act
        let result = push_expected_key(&mut keys, second, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        assert_eq!(keys.len(), 1);
        assert_eq!(controls.current_query_memory_bytes(), inline + slot);
        drop(keys);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_grow_verification_key_slots_geometrically() {
        // Arrange
        let controls = controls(1024 * 1024);
        let mut keys = ExpectedKeys::try_new(&controls).expect("empty keys");

        // Act
        let mut growths = 0;
        for _ in 0..1024 {
            let previous_capacity = keys.values.capacity();
            let key = Accounted::try_new(&controls, size_of::<Vec<u8>>(), Vec::new).expect("key");
            push_expected_key(&mut keys, key, &controls).expect("slot");
            growths += usize::from(keys.values.capacity() != previous_capacity);
        }
        let (values, memory) = keys.into_parts();

        // Assert
        assert!(growths <= 32, "geometric slots avoid per-key growth");
        assert!(values.capacity() > values.len());
        assert!(values.capacity() <= 2 * values.len());
        assert_eq!(memory.bytes(), values.capacity() * size_of::<ExpectedKey>());
        drop(values);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_deny_the_large_verification_slot_replacement_before_growth() {
        // Arrange
        let slot = size_of::<ExpectedKey>();
        let inline = size_of::<Vec<u8>>();
        let controls = controls(212 * inline + (211 + 316) * slot - 1);
        let mut keys = ExpectedKeys::try_new(&controls).expect("empty keys");
        for _ in 0..211 {
            let key = Accounted::try_new(&controls, inline, Vec::new).expect("existing key");
            push_expected_key(&mut keys, key, &controls).expect("existing slots");
        }
        let candidate = Accounted::try_new(&controls, inline, Vec::new).expect("candidate key");

        // Act
        let result = push_expected_key(&mut keys, candidate, &controls);

        // Assert
        assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        assert_eq!(keys.len(), 211);
        assert_eq!(keys.values.capacity(), 211);
        assert_eq!(controls.current_query_memory_bytes(), 211 * (inline + slot));
        drop(keys);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_account_actual_graph_keys_for_long_escaped_numeric_identities() {
        // Arrange
        let graph = GraphMeta::new("key-bound");
        let identity = "\0Ékey".repeat(4096);
        let controls = controls(8 * 1024 * 1024);
        for payload in [
            serde_json::json!({"edge_id":identity, "source_type":identity, "source_id":identity, "target_type":identity, "target_id":identity, "edge_type":identity, "weight":1}),
            serde_json::json!({"edge_id":-1.797_693_134_862_315_7e308_f64, "source_type":i64::MIN, "source_id":u64::MAX, "target_type":-0.0, "target_id":1e-300, "edge_type":1e300, "weight":1}),
        ] {
            let bytes =
                record_construction_bytes(&graph, &identity, &payload).expect("record admission");
            let record_memory = controls
                .reserve_query_memory(bytes)
                .expect("reserve record");

            // Act
            let record = graph_edge_record_from_payload(&graph, &identity, &payload, true)
                .expect("record")
                .expect("complete record");
            let actual = size_of::<GraphEdgeRecord>()
                + [
                    &record.graph,
                    &record.row_id,
                    &record.edge_id,
                    &record.source_type,
                    &record.source_id,
                    &record.target_type,
                    &record.target_id,
                    &record.edge_type,
                ]
                .iter()
                .map(|value| value.capacity())
                .sum::<usize>();
            let keys = [
                key_encoding::graph_outbound_edge_key,
                key_encoding::graph_inbound_edge_key,
                key_encoding::graph_outbound_edge_type_key,
                key_encoding::graph_inbound_edge_type_key,
            ]
            .into_iter()
            .map(|constructor| {
                build_accounted_key(&record, constructor, &controls).expect("admitted key")
            })
            .collect::<Vec<_>>();

            // Assert
            assert!(
                actual <= bytes,
                "record allocated {actual}, admitted {bytes}"
            );
            for key in &keys {
                assert_eq!(
                    key.accounted_bytes(),
                    size_of::<Vec<u8>>() + key.get().capacity()
                );
            }
            drop(keys);
            drop(record);
            drop(record_memory);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
