use cntryl_lexkey::{Encoder, LexKey};

use crate::app::CassieError;

pub(super) const LAYOUT_VERSION: &str = "2";
pub(super) const LAYOUT_MARKER_VALUE: &[u8] = b"cassie-midge-lexkey-v2";

const ROOT: &[u8] = b"cassie";
const LEXKEY: &[u8] = b"lexkey";
const VERSION: &[u8] = b"v2";

const FAMILY_LAYOUT: &[u8] = b"layout";
const FAMILY_COLLECTION_SCHEMA: &[u8] = b"schema";
const FAMILY_ROW_SCHEMA: &[u8] = b"row-schema";
const FAMILY_PROJECTION: &[u8] = b"projection";
const FAMILY_PROJECTION_COMPARISON_REPORT: &[u8] = b"projection-comparison-report";
const FAMILY_PROJECTION_CONSISTENCY_REPORT: &[u8] = b"projection-consistency-report";
const FAMILY_PROJECTION_EVENT: &[u8] = b"projection-event";
const FAMILY_PROJECTION_REPAIR_REPORT: &[u8] = b"projection-repair-report";
const FAMILY_OPERATIONAL_ASSIGNMENT: &[u8] = b"operational-assignment";
const FAMILY_ROW_HASH: &[u8] = b"row-hash";
const FAMILY_RANGE_HASH: &[u8] = b"range-hash";
const FAMILY_ROOT_HASH: &[u8] = b"root-hash";
const FAMILY_VECTOR_INDEX: &[u8] = b"vector-index";
const FAMILY_NORMALIZED_VECTOR: &[u8] = b"normalized-vector";
const FAMILY_INDEX: &[u8] = b"index";
const FAMILY_SCALAR_INDEX: &[u8] = b"scalar-index";
const FAMILY_TIME_SERIES_INDEX: &[u8] = b"time-series-index";
const FAMILY_COLUMN_BATCH: &[u8] = b"column-batch";
const FAMILY_FUNCTION: &[u8] = b"function";
const FAMILY_PROCEDURE: &[u8] = b"procedure";
const FAMILY_VIEW: &[u8] = b"view";
const FAMILY_ROLE: &[u8] = b"role";
const FAMILY_SEQUENCE: &[u8] = b"sequence";
const FAMILY_CONSTRAINTS: &[u8] = b"constraints";
const FAMILY_NAMESPACE: &[u8] = b"namespace";
const FAMILY_NAMESPACES: &[u8] = b"namespaces";
const FAMILY_SCHEMA_EPOCH: &[u8] = b"schema-epoch";
const FAMILY_SCHEMA_CLEANUP: &[u8] = b"schema-cleanup";
const FAMILY_COLLECTIONS: &[u8] = b"collections";
const FAMILY_ROW: &[u8] = b"row";
const FAMILY_LEGACY_DOC: &[u8] = b"legacy-doc";
const FAMILY_CARDINALITY: &[u8] = b"cardinality";
const FAMILY_OPERATOR_FEEDBACK: &[u8] = b"operator-feedback";
const FAMILY_COLLECTION_METADATA: &[u8] = b"collection-meta";
const FAMILY_COLUMN_STORE: &[u8] = b"column-store";
const FAMILY_ROLLUP: &[u8] = b"rollup";
const FAMILY_RETENTION: &[u8] = b"retention";
const FAMILY_GRAPH: &[u8] = b"graph";
const FAMILY_GRAPH_ADJACENCY: &[u8] = b"graph-adjacency";

pub(super) const LEGACY_SCHEMA_PREFIXES: &[&[u8]] = &[b"__cassie__/", b"r/", b"doc:"];

pub(super) const LEGACY_DATA_PREFIXES: &[&[u8]] = &[b"__cassie__/", b"r/", b"doc:"];

pub(super) const LEGACY_TEMP_PREFIXES: &[&[u8]] = &[b"__cassie__/"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CapacityKeyKind {
    RowBlob,
    ScalarIndex,
    IndexMetadata,
    VectorSidecar,
    ColumnBatch,
    ProjectionMetadata,
    TempArtifact,
    Other,
}

#[derive(Debug, Clone)]
pub(super) struct CapacityKeyPrefix {
    pub kind: CapacityKeyKind,
    pub prefix: Vec<u8>,
}

pub(super) fn capacity_key_prefixes() -> Vec<CapacityKeyPrefix> {
    [
        (CapacityKeyKind::RowBlob, FAMILY_ROW),
        (CapacityKeyKind::RowBlob, FAMILY_LEGACY_DOC),
        (CapacityKeyKind::ScalarIndex, FAMILY_SCALAR_INDEX),
        (CapacityKeyKind::ScalarIndex, FAMILY_TIME_SERIES_INDEX),
        (CapacityKeyKind::IndexMetadata, FAMILY_INDEX),
        (CapacityKeyKind::VectorSidecar, FAMILY_VECTOR_INDEX),
        (CapacityKeyKind::VectorSidecar, FAMILY_NORMALIZED_VECTOR),
        (CapacityKeyKind::ColumnBatch, FAMILY_COLUMN_BATCH),
        (CapacityKeyKind::ColumnBatch, FAMILY_COLUMN_STORE),
        (CapacityKeyKind::ProjectionMetadata, FAMILY_PROJECTION),
        (
            CapacityKeyKind::ProjectionMetadata,
            FAMILY_PROJECTION_COMPARISON_REPORT,
        ),
        (
            CapacityKeyKind::ProjectionMetadata,
            FAMILY_PROJECTION_CONSISTENCY_REPORT,
        ),
        (CapacityKeyKind::ProjectionMetadata, FAMILY_PROJECTION_EVENT),
        (
            CapacityKeyKind::ProjectionMetadata,
            FAMILY_PROJECTION_REPAIR_REPORT,
        ),
        (
            CapacityKeyKind::ProjectionMetadata,
            FAMILY_OPERATIONAL_ASSIGNMENT,
        ),
        (CapacityKeyKind::ProjectionMetadata, FAMILY_ROW_HASH),
        (CapacityKeyKind::ProjectionMetadata, FAMILY_RANGE_HASH),
        (CapacityKeyKind::ProjectionMetadata, FAMILY_ROOT_HASH),
        (CapacityKeyKind::TempArtifact, FAMILY_OPERATOR_FEEDBACK),
    ]
    .into_iter()
    .map(|(kind, family)| CapacityKeyPrefix {
        kind,
        prefix: prefix(family, &[]),
    })
    .collect()
}

pub(super) fn layout_marker_key() -> Vec<u8> {
    key(FAMILY_LAYOUT, &[b"version"])
}

pub(super) fn collection_schema_key(collection: &str) -> Vec<u8> {
    key(FAMILY_COLLECTION_SCHEMA, &[collection.as_bytes()])
}

pub(super) fn row_schema_key(collection: &str) -> Vec<u8> {
    key(FAMILY_ROW_SCHEMA, &[collection.as_bytes()])
}

pub(super) fn projection_key(collection: &str) -> Vec<u8> {
    key(FAMILY_PROJECTION, &[collection.as_bytes()])
}

pub(super) fn projection_prefix() -> Vec<u8> {
    prefix(FAMILY_PROJECTION, &[])
}

pub(super) fn projection_comparison_report_key(report_id: &str) -> Vec<u8> {
    key(FAMILY_PROJECTION_COMPARISON_REPORT, &[report_id.as_bytes()])
}

pub(super) fn projection_comparison_report_prefix() -> Vec<u8> {
    prefix(FAMILY_PROJECTION_COMPARISON_REPORT, &[])
}

pub(super) fn projection_consistency_report_key(report_id: &str) -> Vec<u8> {
    key(
        FAMILY_PROJECTION_CONSISTENCY_REPORT,
        &[report_id.as_bytes()],
    )
}

pub(super) fn projection_consistency_report_prefix() -> Vec<u8> {
    prefix(FAMILY_PROJECTION_CONSISTENCY_REPORT, &[])
}

pub(super) fn projection_event_key(
    projection: &str,
    source_identity: &str,
    event_id: &str,
) -> Vec<u8> {
    key(
        FAMILY_PROJECTION_EVENT,
        &[
            projection.as_bytes(),
            source_identity.as_bytes(),
            event_id.as_bytes(),
        ],
    )
}

pub(super) fn projection_event_prefix(projection: &str) -> Vec<u8> {
    prefix(FAMILY_PROJECTION_EVENT, &[projection.as_bytes()])
}

pub(super) fn projection_repair_report_key(report_id: &str) -> Vec<u8> {
    key(FAMILY_PROJECTION_REPAIR_REPORT, &[report_id.as_bytes()])
}

pub(super) fn projection_repair_report_prefix() -> Vec<u8> {
    prefix(FAMILY_PROJECTION_REPAIR_REPORT, &[])
}

pub(super) fn operational_assignment_key(assignment_id: &str) -> Vec<u8> {
    key(FAMILY_OPERATIONAL_ASSIGNMENT, &[assignment_id.as_bytes()])
}

pub(super) fn operational_assignment_prefix() -> Vec<u8> {
    prefix(FAMILY_OPERATIONAL_ASSIGNMENT, &[])
}

pub(super) fn row_hash_key(collection: &str, row_id: &str) -> Vec<u8> {
    key(FAMILY_ROW_HASH, &[collection.as_bytes(), row_id.as_bytes()])
}

pub(super) fn row_hash_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_ROW_HASH, &[collection.as_bytes()])
}

pub(super) fn range_hash_key(collection: &str, range_id: u64) -> Vec<u8> {
    let encoded_range_id = LexKey::encode_u64(range_id);
    key(
        FAMILY_RANGE_HASH,
        &[collection.as_bytes(), encoded_range_id.as_bytes()],
    )
}

pub(super) fn range_hash_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_RANGE_HASH, &[collection.as_bytes()])
}

pub(super) fn root_hash_key(collection: &str) -> Vec<u8> {
    key(FAMILY_ROOT_HASH, &[collection.as_bytes()])
}

pub(super) fn schema_collection_prefix() -> Vec<u8> {
    prefix(FAMILY_COLLECTION_SCHEMA, &[])
}

pub(super) fn vector_index_key(collection: &str, field: &str) -> Vec<u8> {
    key(
        FAMILY_VECTOR_INDEX,
        &[collection.as_bytes(), field.as_bytes()],
    )
}

pub(super) fn vector_index_prefix() -> Vec<u8> {
    prefix(FAMILY_VECTOR_INDEX, &[])
}

pub(super) fn vector_index_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_VECTOR_INDEX, &[collection.as_bytes()])
}

pub(super) fn normalized_vector_key(collection: &str, field: &str, id: &str) -> Vec<u8> {
    key(
        FAMILY_NORMALIZED_VECTOR,
        &[collection.as_bytes(), field.as_bytes(), id.as_bytes()],
    )
}

pub(super) fn normalized_vector_prefix(collection: &str, field: &str) -> Vec<u8> {
    prefix(
        FAMILY_NORMALIZED_VECTOR,
        &[collection.as_bytes(), field.as_bytes()],
    )
}

pub(super) fn normalized_vector_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_NORMALIZED_VECTOR, &[collection.as_bytes()])
}

pub(super) fn index_key(collection: &str, name: &str) -> Vec<u8> {
    key(FAMILY_INDEX, &[collection.as_bytes(), name.as_bytes()])
}

pub(super) fn index_prefix() -> Vec<u8> {
    prefix(FAMILY_INDEX, &[])
}

pub(super) fn index_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_INDEX, &[collection.as_bytes()])
}

pub(super) fn schema_cleanup_key(cleanup_id: &str) -> Vec<u8> {
    key(FAMILY_SCHEMA_CLEANUP, &[cleanup_id.as_bytes()])
}

pub(super) fn schema_cleanup_prefix() -> Vec<u8> {
    prefix(FAMILY_SCHEMA_CLEANUP, &[])
}

pub(super) fn scalar_index_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_SCALAR_INDEX, &[collection.as_bytes()])
}

pub(super) fn scalar_index_data_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    prefix(
        FAMILY_SCALAR_INDEX,
        &[collection.as_bytes(), index_name.as_bytes(), b"data"],
    )
}

pub(super) fn time_series_index_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_TIME_SERIES_INDEX, &[collection.as_bytes()])
}

pub(super) fn time_series_index_data_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    prefix(
        FAMILY_TIME_SERIES_INDEX,
        &[collection.as_bytes(), index_name.as_bytes(), b"data"],
    )
}

pub(super) fn column_batch_metadata_key(collection: &str, index_name: &str) -> Vec<u8> {
    key(
        FAMILY_COLUMN_BATCH,
        &[collection.as_bytes(), index_name.as_bytes(), b"metadata"],
    )
}

pub(super) fn column_batch_segment_key(
    collection: &str,
    index_name: &str,
    segment_id: u64,
) -> Vec<u8> {
    let encoded_segment = LexKey::encode_u64(segment_id);
    key(
        FAMILY_COLUMN_BATCH,
        &[
            collection.as_bytes(),
            index_name.as_bytes(),
            b"segment",
            encoded_segment.as_bytes(),
        ],
    )
}

pub(super) fn column_batch_index_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    prefix(
        FAMILY_COLUMN_BATCH,
        &[collection.as_bytes(), index_name.as_bytes()],
    )
}

pub(super) fn column_batch_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_COLUMN_BATCH, &[collection.as_bytes()])
}

pub(super) fn function_key(name: &str) -> Vec<u8> {
    key(FAMILY_FUNCTION, &[name.to_ascii_lowercase().as_bytes()])
}

pub(super) fn function_prefix() -> Vec<u8> {
    prefix(FAMILY_FUNCTION, &[])
}

pub(super) fn procedure_key(name: &str) -> Vec<u8> {
    key(FAMILY_PROCEDURE, &[name.to_ascii_lowercase().as_bytes()])
}

pub(super) fn procedure_prefix() -> Vec<u8> {
    prefix(FAMILY_PROCEDURE, &[])
}

pub(super) fn view_key(name: &str) -> Vec<u8> {
    key(FAMILY_VIEW, &[name.as_bytes()])
}

pub(super) fn view_prefix() -> Vec<u8> {
    prefix(FAMILY_VIEW, &[])
}

pub(super) fn role_key(name: &str) -> Vec<u8> {
    key(FAMILY_ROLE, &[name.to_ascii_lowercase().as_bytes()])
}

pub(super) fn role_prefix() -> Vec<u8> {
    prefix(FAMILY_ROLE, &[])
}

pub(super) fn sequence_key(name: &str) -> Vec<u8> {
    key(FAMILY_SEQUENCE, &[name.as_bytes()])
}

pub(super) fn sequence_prefix() -> Vec<u8> {
    prefix(FAMILY_SEQUENCE, &[])
}

pub(super) fn constraints_key(collection: &str) -> Vec<u8> {
    key(FAMILY_CONSTRAINTS, &[collection.as_bytes()])
}

pub(super) fn namespace_key(namespace: &str) -> Vec<u8> {
    key(FAMILY_NAMESPACE, &[namespace.as_bytes()])
}

pub(super) fn namespace_prefix() -> Vec<u8> {
    prefix(FAMILY_NAMESPACE, &[])
}

pub(super) fn namespaces_key() -> Vec<u8> {
    key(FAMILY_NAMESPACES, &[])
}

pub(super) fn schema_epoch_key() -> Vec<u8> {
    key(FAMILY_SCHEMA_EPOCH, &[])
}

pub(super) fn collections_key() -> Vec<u8> {
    key(FAMILY_COLLECTIONS, &[])
}

pub(super) fn row_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_ROW, &[collection.as_bytes()])
}

pub(super) fn row_key(collection: &str, id: &str) -> Vec<u8> {
    key(FAMILY_ROW, &[collection.as_bytes(), id.as_bytes()])
}

pub(super) fn doc_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_LEGACY_DOC, &[collection.as_bytes()])
}

pub(super) fn doc_key(collection: &str, id: &str) -> Vec<u8> {
    key(FAMILY_LEGACY_DOC, &[collection.as_bytes(), id.as_bytes()])
}

pub(super) fn cardinality_key(collection: &str) -> Vec<u8> {
    key(FAMILY_CARDINALITY, &[collection.as_bytes()])
}

pub(super) fn cardinality_prefix() -> Vec<u8> {
    prefix(FAMILY_CARDINALITY, &[])
}

pub(super) fn runtime_feedback_key(fingerprint: u64) -> Vec<u8> {
    let encoded = LexKey::encode_u64(fingerprint);
    key(FAMILY_OPERATOR_FEEDBACK, &[encoded.as_bytes()])
}

pub(super) fn runtime_feedback_prefix() -> Vec<u8> {
    prefix(FAMILY_OPERATOR_FEEDBACK, &[])
}

pub(super) fn collection_metadata_key(name: &str) -> Vec<u8> {
    key(FAMILY_COLLECTION_METADATA, &[name.as_bytes()])
}

pub(super) fn column_store_collection_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_COLUMN_STORE, &[collection.as_bytes()])
}

pub(super) fn column_store_row_prefix(collection: &str) -> Vec<u8> {
    prefix(FAMILY_COLUMN_STORE, &[collection.as_bytes(), b"row"])
}

pub(super) fn column_store_row_key(collection: &str, id: &str) -> Vec<u8> {
    key(
        FAMILY_COLUMN_STORE,
        &[collection.as_bytes(), b"row", id.as_bytes()],
    )
}

pub(super) fn column_store_deleted_key(collection: &str, id: &str) -> Vec<u8> {
    key(
        FAMILY_COLUMN_STORE,
        &[collection.as_bytes(), b"deleted", id.as_bytes()],
    )
}

pub(super) fn column_store_field_key(collection: &str, field: &str, id: &str) -> Vec<u8> {
    key(
        FAMILY_COLUMN_STORE,
        &[
            collection.as_bytes(),
            b"field",
            field.as_bytes(),
            id.as_bytes(),
        ],
    )
}

pub(super) fn rollup_prefix() -> Vec<u8> {
    prefix(FAMILY_ROLLUP, &[])
}

pub(super) fn rollup_key(name: &str) -> Vec<u8> {
    key(
        FAMILY_ROLLUP,
        &[name.trim().to_ascii_lowercase().as_bytes()],
    )
}

pub(super) fn retention_prefix() -> Vec<u8> {
    prefix(FAMILY_RETENTION, &[])
}

pub(super) fn retention_key(name: &str) -> Vec<u8> {
    key(
        FAMILY_RETENTION,
        &[name.trim().to_ascii_lowercase().as_bytes()],
    )
}

pub(super) fn graph_prefix() -> Vec<u8> {
    prefix(FAMILY_GRAPH, &[])
}

pub(super) fn graph_key(name: &str) -> Vec<u8> {
    key(FAMILY_GRAPH, &[name.trim().to_ascii_lowercase().as_bytes()])
}

pub(super) fn graph_outbound_prefix(graph: &str, source_type: &str, source_id: &str) -> Vec<u8> {
    prefix(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_bytes(),
            b"out",
            source_type.as_bytes(),
            source_id.as_bytes(),
        ],
    )
}

pub(super) fn graph_inbound_prefix(graph: &str, target_type: &str, target_id: &str) -> Vec<u8> {
    prefix(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_bytes(),
            b"in",
            target_type.as_bytes(),
            target_id.as_bytes(),
        ],
    )
}

pub(super) fn graph_outbound_edge_key(
    graph: &str,
    source_type: &str,
    source_id: &str,
    edge_type: &str,
    target_type: &str,
    target_id: &str,
    edge_id: &str,
) -> Vec<u8> {
    key(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_bytes(),
            b"out",
            source_type.as_bytes(),
            source_id.as_bytes(),
            edge_type.as_bytes(),
            target_type.as_bytes(),
            target_id.as_bytes(),
            edge_id.as_bytes(),
        ],
    )
}

pub(super) fn graph_inbound_edge_key(
    graph: &str,
    target_type: &str,
    target_id: &str,
    edge_type: &str,
    source_type: &str,
    source_id: &str,
    edge_id: &str,
) -> Vec<u8> {
    key(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_bytes(),
            b"in",
            target_type.as_bytes(),
            target_id.as_bytes(),
            edge_type.as_bytes(),
            source_type.as_bytes(),
            source_id.as_bytes(),
            edge_id.as_bytes(),
        ],
    )
}

pub(super) fn utf8_suffix_after_prefix(key: &[u8], prefix: &[u8]) -> Option<String> {
    std::str::from_utf8(key.strip_prefix(prefix)?)
        .ok()
        .map(ToString::to_string)
}

pub(super) fn scalar_index_entry_key(
    collection: &str,
    index_name: &str,
    values: &[serde_json::Value],
    id: &str,
) -> Result<Vec<u8>, CassieError> {
    let mut key = scalar_index_data_prefix(collection, index_name);
    for value in values {
        append_scalar_value(&mut key, value)?;
    }
    key.push(LexKey::SEPARATOR);
    append_terminated_component(&mut key, id.as_bytes());
    Ok(key)
}

pub(super) fn time_series_index_entry_key(
    collection: &str,
    index_name: &str,
    bucket_key: &str,
    id: &str,
) -> Vec<u8> {
    key(
        FAMILY_TIME_SERIES_INDEX,
        &[
            collection.as_bytes(),
            index_name.as_bytes(),
            b"data",
            bucket_key.as_bytes(),
            id.as_bytes(),
        ],
    )
}

pub(super) fn scalar_index_seek_prefix(
    collection: &str,
    index_name: &str,
    equality_prefix: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let mut seek_prefix = scalar_index_data_prefix(collection, index_name);
    for value in equality_prefix {
        append_scalar_value(&mut seek_prefix, value)?;
    }
    Ok(seek_prefix)
}

#[allow(clippy::type_complexity)]
pub(super) fn scalar_index_query_bounds(
    seek_prefix: &[u8],
    lower_bound: Option<&super::ScalarIndexBound>,
    upper_bound: Option<&super::ScalarIndexBound>,
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>), CassieError> {
    if lower_bound.is_none() && upper_bound.is_none() && seek_prefix.is_empty() {
        return Ok((None, None));
    }

    let start_key = match lower_bound {
        Some(bound) => Some(scalar_index_bound_key(
            seek_prefix,
            &bound.value,
            bound.inclusive,
            true,
        )?),
        None if seek_prefix.is_empty() => None,
        None => Some(seek_prefix.to_vec()),
    };
    let end_key = match upper_bound {
        Some(bound) => Some(scalar_index_bound_key(
            seek_prefix,
            &bound.value,
            bound.inclusive,
            false,
        )?),
        None if seek_prefix.is_empty() => None,
        None => Some(LexKey::prefix_end(seek_prefix)),
    };
    Ok((start_key, end_key))
}

fn scalar_index_bound_key(
    seek_prefix: &[u8],
    value: &serde_json::Value,
    inclusive: bool,
    lower: bool,
) -> Result<Vec<u8>, CassieError> {
    let mut key = seek_prefix.to_vec();
    append_scalar_value(&mut key, value)?;
    if (lower && !inclusive) || (!lower && inclusive) {
        key.push(LexKey::END_MARKER);
    }
    Ok(key)
}

fn key(family: &[u8], components: &[&[u8]]) -> Vec<u8> {
    encode(family, components, false)
}

fn prefix(family: &[u8], components: &[&[u8]]) -> Vec<u8> {
    encode(family, components, true)
}

fn encode(family: &[u8], components: &[&[u8]], trailing_separator: bool) -> Vec<u8> {
    let mut parts = Vec::with_capacity(4 + components.len());
    parts.push(ROOT);
    parts.push(LEXKEY);
    parts.push(VERSION);
    parts.push(family);
    parts.extend_from_slice(components);

    let capacity = parts.iter().map(|part| part.len()).sum::<usize>() + parts.len();
    let mut encoder = Encoder::with_capacity(capacity);
    encoder.encode_composite_into_buf(&parts);
    if trailing_separator {
        encoder.push_separator();
    }
    encoder.into_vec()
}

fn append_scalar_value(key: &mut Vec<u8>, value: &serde_json::Value) -> Result<(), CassieError> {
    let mut encoder = Encoder::with_capacity(10);
    match value {
        serde_json::Value::Null => key.extend_from_slice(&[0x10, LexKey::SEPARATOR]),
        serde_json::Value::Bool(value) => {
            key.push(0x20);
            encoder.encode_u8_into(u8::from(*value));
            key.extend_from_slice(encoder.as_slice());
            key.push(LexKey::SEPARATOR);
        }
        serde_json::Value::Number(number) => {
            if let Some(integer) = number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            {
                key.push(0x30);
                encoder.encode_i64_into(integer);
                key.extend_from_slice(encoder.as_slice());
                key.push(LexKey::SEPARATOR);
            } else if let Some(float) = number.as_f64() {
                if !float.is_finite() {
                    return Err(CassieError::Unsupported(
                        "non-finite scalar index values are not supported".to_string(),
                    ));
                }
                key.push(0x40);
                encoder.encode_f64_into(float);
                key.extend_from_slice(encoder.as_slice());
                key.push(LexKey::SEPARATOR);
            } else {
                return Err(CassieError::Unsupported(
                    "scalar index number is not representable".to_string(),
                ));
            }
        }
        serde_json::Value::String(value) => {
            key.push(0x50);
            append_terminated_component(key, value.as_bytes());
        }
        other => {
            return Err(CassieError::Unsupported(format!(
                "scalar index does not support key value '{other}'"
            )));
        }
    }
    Ok(())
}

fn append_terminated_component(key: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        match *byte {
            LexKey::SEPARATOR => {
                key.push(LexKey::SEPARATOR);
                key.push(LexKey::END_MARKER);
            }
            value => key.push(value),
        }
    }
    key.push(LexKey::SEPARATOR);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn should_build_deterministic_lexkey_storage_keys() {
        // Arrange
        let left = row_key("events", "id-1");

        // Act
        let right = row_key("events", "id-1");
        let other_family = row_hash_key("events", "id-1");

        // Assert
        assert_eq!(left, right);
        assert_ne!(left, other_family);
        assert!(!left.starts_with(b"__cassie__/"));
        assert!(!left.starts_with(b"r/"));
    }

    #[test]
    fn should_build_prefix_that_matches_only_child_keys() {
        // Arrange
        let prefix = row_prefix("orders");
        let matching = row_key("orders", "1");
        let sibling = row_key("orders-archive", "1");

        // Act
        let decoded = utf8_suffix_after_prefix(&matching, &prefix);

        // Assert
        assert!(matching.starts_with(&prefix));
        assert!(!sibling.starts_with(&prefix));
        assert_eq!(decoded.as_deref(), Some("1"));
    }

    #[test]
    fn should_preserve_scalar_value_ordering() {
        // Arrange
        let values = vec![
            json!(null),
            json!(false),
            json!(true),
            json!(-10),
            json!(0),
            json!(7),
            json!(-1.25),
            json!(2.5),
            json!("a\u{0}a"),
            json!("aa"),
        ];

        // Act
        let encoded = values
            .iter()
            .map(|value| {
                let mut key = Vec::new();
                append_scalar_value(&mut key, value).expect("scalar value");
                key
            })
            .collect::<Vec<_>>();
        let mut sorted = encoded.clone();
        sorted.sort();

        // Assert
        assert_eq!(encoded, sorted);
    }

    #[test]
    fn should_reject_unsupported_scalar_value_without_panicking() {
        // Arrange
        let value = json!([]);
        let mut key = Vec::new();

        // Act
        let result = append_scalar_value(&mut key, &value);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_include_embedded_nul_text_in_scalar_order() {
        // Arrange
        let before = scalar_index_entry_key("events", "idx", &[json!("a\u{0}a")], "1").unwrap();
        let after = scalar_index_entry_key("events", "idx", &[json!("aa")], "1").unwrap();

        // Act / Assert
        assert!(before < after);
    }
}
