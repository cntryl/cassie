use cntryl_lexkey::{Encoder, LexKey};

use crate::app::CassieError;
use crate::catalog::split_identifier_path;

#[path = "key_encoding/fulltext.rs"]
mod fulltext;
#[path = "key_encoding/graph.rs"]
mod graph;
#[path = "key_encoding/vector.rs"]
mod vector;

pub(crate) use fulltext::{
    fulltext_document_stats_key, fulltext_document_stats_prefix, fulltext_index_artifact_prefix,
    fulltext_index_collection_prefix, fulltext_index_key, fulltext_index_manifest_key,
    fulltext_postings_prefix, fulltext_term_postings_key,
};
pub(super) use graph::{
    graph_adjacency_prefix, graph_inbound_edge_key, graph_inbound_prefix, graph_key,
    graph_outbound_edge_key, graph_outbound_prefix, graph_prefix,
};
pub(super) use vector::{
    decode_ivfflat_membership_suffix, ivfflat_membership_key, ivfflat_membership_list_prefix,
    ivfflat_membership_prefix, ivfflat_source_summary_key,
};

pub(super) const LAYOUT_VERSION: &str = "5";
pub(super) const LAYOUT_MARKER_VALUE: &[u8] = b"cassie-midge-lexkey-v5";

const ROOT: &[u8] = b"cassie";
const LEXKEY: &[u8] = b"lexkey";
const VERSION: &[u8] = b"v5";
const LEGACY_VERSION_V2: &[u8] = b"v2";

const FAMILY_LAYOUT: &[u8] = b"layout";
const FAMILY_DATABASE: &[u8] = b"database";
const FAMILY_DATABASES: &[u8] = b"databases";
const FAMILY_DATABASE_LIFECYCLE: &[u8] = b"database-lifecycle";
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
const FAMILY_VECTOR_INDEX_STATE: &[u8] = b"vector-index-state";
const FAMILY_NORMALIZED_VECTOR: &[u8] = b"normalized-vector";
const FAMILY_INDEX: &[u8] = b"index";
const FAMILY_FULLTEXT_INDEX: &[u8] = b"fulltext-index";
const FAMILY_SCALAR_INDEX: &[u8] = b"scalar-index";
const FAMILY_TIME_SERIES_INDEX: &[u8] = b"time-series-index";
const FAMILY_UNIQUE_RESERVATION: &[u8] = b"unique-reservation";
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
const FAMILY_INDEX_PUBLICATION: &[u8] = b"index-publication";
const FAMILY_SCHEMA_OPERATION: &[u8] = b"schema-operation";
const FAMILY_FIELD_ADD_OPERATION: &[u8] = b"field-add-operation";
const FAMILY_FIELD_RENAME_OPERATION: &[u8] = b"field-rename-operation";
const FAMILY_FIELD_DROP_OPERATION: &[u8] = b"field-drop-operation";
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
const FAMILY_DATA_EPOCH: &[u8] = b"data-epoch";
const FAMILY_COLLECTION_GENERATION: &[u8] = b"collection-generation";
const FAMILY_MAINTENANCE_DEBT: &[u8] = b"maintenance-debt";

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum FulltextArtifactKind {
    Meta = 1,
    Manifest = 2,
    Postings = 3,
    Document = 4,
}

impl FulltextArtifactKind {
    const fn as_segment(self) -> [u8; 1] {
        match self {
            Self::Meta => [Self::Meta as u8],
            Self::Manifest => [Self::Manifest as u8],
            Self::Postings => [Self::Postings as u8],
            Self::Document => [Self::Document as u8],
        }
    }
}

pub(crate) const FULLTEXT_ARTIFACT_META: [u8; 1] = FulltextArtifactKind::Meta.as_segment();
pub(crate) const FULLTEXT_ARTIFACT_MANIFEST: [u8; 1] = FulltextArtifactKind::Manifest.as_segment();
pub(crate) const FULLTEXT_ARTIFACT_POSTINGS: [u8; 1] = FulltextArtifactKind::Postings.as_segment();
pub(crate) const FULLTEXT_ARTIFACT_DOCUMENT: [u8; 1] = FulltextArtifactKind::Document.as_segment();

pub(super) const LEGACY_SCHEMA_PREFIXES: &[&[u8]] = &[b"__cassie__/", b"r/", b"doc:"];

pub(super) const LEGACY_DATA_PREFIXES: &[&[u8]] = &[b"__cassie__/", b"r/", b"doc:"];

pub(super) const LEGACY_TEMP_PREFIXES: &[&[u8]] = &[b"__cassie__/"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CapacityKeyKind {
    RowBlob,
    ScalarIndex,
    FullTextMetadata,
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
        (CapacityKeyKind::IndexMetadata, FAMILY_UNIQUE_RESERVATION),
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
        (CapacityKeyKind::FullTextMetadata, FAMILY_FULLTEXT_INDEX),
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

pub(super) fn data_epoch_key() -> Vec<u8> {
    key(FAMILY_DATA_EPOCH, &[])
}

pub(super) fn collection_generation_key(collection: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_COLLECTION_GENERATION, collection, &[])
}

pub(super) fn maintenance_debt_key(collection: &str, artifact: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_MAINTENANCE_DEBT, collection, &[artifact.as_bytes()])
}

pub(super) fn maintenance_debt_prefix() -> Vec<u8> {
    prefix(FAMILY_MAINTENANCE_DEBT, &[])
}

pub(super) fn legacy_v2_layout_prefix() -> Vec<u8> {
    let parts = [ROOT, LEXKEY, LEGACY_VERSION_V2];
    let mut encoder =
        Encoder::with_capacity(parts.iter().map(|part| part.len()).sum::<usize>() + 4);
    encoder.encode_composite_into_buf(&parts);
    encoder.push_separator();
    encoder.into_vec()
}

pub(super) fn database_key(name: &str) -> Vec<u8> {
    key(FAMILY_DATABASE, &[name.as_bytes()])
}

pub(super) fn database_prefix() -> Vec<u8> {
    prefix(FAMILY_DATABASE, &[])
}

pub(super) fn databases_key() -> Vec<u8> {
    key(FAMILY_DATABASES, &[])
}

pub(super) fn database_lifecycle_key(operation_id: &str) -> Vec<u8> {
    key(FAMILY_DATABASE_LIFECYCLE, &[operation_id.as_bytes()])
}

pub(super) fn database_lifecycle_prefix() -> Vec<u8> {
    prefix(FAMILY_DATABASE_LIFECYCLE, &[])
}

pub(super) fn collection_schema_key(collection: &str) -> Vec<u8> {
    scoped_key(FAMILY_COLLECTION_SCHEMA, collection, &[])
}

pub(super) fn row_schema_key(collection: &str) -> Vec<u8> {
    scoped_key(FAMILY_ROW_SCHEMA, collection, &[])
}

pub(super) fn projection_key(collection: &str) -> Vec<u8> {
    scoped_key(FAMILY_PROJECTION, collection, &[])
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
    scoped_key(
        FAMILY_PROJECTION_EVENT,
        projection,
        &[source_identity.as_bytes(), event_id.as_bytes()],
    )
}

pub(super) fn projection_event_prefix(projection: &str) -> Vec<u8> {
    scoped_prefix(FAMILY_PROJECTION_EVENT, projection, &[])
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
    data_scoped_key(FAMILY_ROW_HASH, collection, &[row_id.as_bytes()])
}

pub(super) fn row_hash_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_ROW_HASH, collection, &[])
}

pub(super) fn range_hash_key(collection: &str, range_id: u64) -> Vec<u8> {
    let encoded_range_id = LexKey::encode_u64(range_id);
    data_scoped_key(
        FAMILY_RANGE_HASH,
        collection,
        &[encoded_range_id.as_bytes()],
    )
}

pub(super) fn range_hash_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_RANGE_HASH, collection, &[])
}

pub(super) fn root_hash_key(collection: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_ROOT_HASH, collection, &[])
}

pub(super) fn schema_collection_prefix() -> Vec<u8> {
    prefix(FAMILY_COLLECTION_SCHEMA, &[])
}

pub(super) fn vector_index_key(collection: &str, field: &str) -> Vec<u8> {
    scoped_key(FAMILY_VECTOR_INDEX, collection, &[field.as_bytes()])
}

pub(super) fn vector_index_prefix() -> Vec<u8> {
    prefix(FAMILY_VECTOR_INDEX, &[])
}

pub(super) fn vector_index_state_key(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_VECTOR_INDEX_STATE, collection, &[field.as_bytes()])
}

pub(super) fn hnsw_graph_node_key(collection: &str, field: &str, id: &str) -> Vec<u8> {
    data_scoped_key(
        FAMILY_VECTOR_INDEX_STATE,
        collection,
        &[field.as_bytes(), b"n", id.as_bytes()],
    )
}

pub(super) fn hnsw_graph_node_prefix(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_VECTOR_INDEX_STATE,
        collection,
        &[field.as_bytes(), b"n"],
    )
}

pub(super) fn hnsw_source_summary_key(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_key(
        FAMILY_VECTOR_INDEX_STATE,
        collection,
        &[field.as_bytes(), b"f"],
    )
}

pub(super) fn vector_index_state_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_VECTOR_INDEX_STATE, collection, &[])
}

pub(super) fn vector_index_collection_prefix(collection: &str) -> Vec<u8> {
    scoped_prefix(FAMILY_VECTOR_INDEX, collection, &[])
}

pub(super) fn normalized_vector_key(collection: &str, field: &str, id: &str) -> Vec<u8> {
    data_scoped_key(
        FAMILY_NORMALIZED_VECTOR,
        collection,
        &[field.as_bytes(), id.as_bytes()],
    )
}

pub(super) fn normalized_vector_prefix(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_NORMALIZED_VECTOR, collection, &[field.as_bytes()])
}

pub(super) fn normalized_vector_collection_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_NORMALIZED_VECTOR, collection, &[])
}

pub(super) fn index_key(collection: &str, name: &str) -> Vec<u8> {
    scoped_key(FAMILY_INDEX, collection, &[name.as_bytes()])
}

pub(super) fn index_prefix() -> Vec<u8> {
    prefix(FAMILY_INDEX, &[])
}

pub(super) fn index_collection_prefix(collection: &str) -> Vec<u8> {
    scoped_prefix(FAMILY_INDEX, collection, &[])
}

pub(super) fn schema_cleanup_key(cleanup_id: &str) -> Vec<u8> {
    key(FAMILY_SCHEMA_CLEANUP, &[cleanup_id.as_bytes()])
}

pub(super) fn schema_cleanup_prefix() -> Vec<u8> {
    prefix(FAMILY_SCHEMA_CLEANUP, &[])
}

pub(super) fn index_publication_key(collection: &str, index: &str) -> Vec<u8> {
    scoped_key(FAMILY_INDEX_PUBLICATION, collection, &[index.as_bytes()])
}

pub(super) fn index_publication_prefix() -> Vec<u8> {
    prefix(FAMILY_INDEX_PUBLICATION, &[])
}

pub(super) fn schema_operation_key(current: &str, next: &str) -> Vec<u8> {
    scoped_key(FAMILY_SCHEMA_OPERATION, current, &[next.as_bytes()])
}

pub(super) fn schema_operation_prefix() -> Vec<u8> {
    prefix(FAMILY_SCHEMA_OPERATION, &[])
}

pub(super) fn field_add_operation_key(collection: &str, field: &str) -> Vec<u8> {
    scoped_key(FAMILY_FIELD_ADD_OPERATION, collection, &[field.as_bytes()])
}

pub(super) fn field_add_operation_prefix() -> Vec<u8> {
    prefix(FAMILY_FIELD_ADD_OPERATION, &[])
}

pub(super) fn field_rename_operation_key(collection: &str, current: &str, next: &str) -> Vec<u8> {
    scoped_key(
        FAMILY_FIELD_RENAME_OPERATION,
        collection,
        &[current.as_bytes(), next.as_bytes()],
    )
}

pub(super) fn field_rename_operation_prefix() -> Vec<u8> {
    prefix(FAMILY_FIELD_RENAME_OPERATION, &[])
}

pub(super) fn field_drop_operation_key(collection: &str, field: &str) -> Vec<u8> {
    scoped_key(FAMILY_FIELD_DROP_OPERATION, collection, &[field.as_bytes()])
}

pub(super) fn field_drop_operation_prefix() -> Vec<u8> {
    prefix(FAMILY_FIELD_DROP_OPERATION, &[])
}

pub(super) fn scalar_index_collection_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_SCALAR_INDEX, collection, &[])
}

pub(super) fn scalar_index_data_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_SCALAR_INDEX,
        collection,
        &[index_name.as_bytes(), b"d"],
    )
}

pub(super) fn time_series_index_collection_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_TIME_SERIES_INDEX, collection, &[])
}

pub(super) fn time_series_index_data_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_TIME_SERIES_INDEX,
        collection,
        &[index_name.as_bytes(), b"d"],
    )
}

pub(super) fn unique_constraint_reservation_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_UNIQUE_RESERVATION, collection, &[b"c"])
}

pub(super) fn unique_constraint_reservation_field_prefix(collection: &str, field: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"c", field.as_bytes()],
    )
}

pub(super) fn unique_constraint_reservation_key(
    collection: &str,
    field: &str,
    value: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    let mut key = data_scoped_key(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"c", field.as_bytes()],
    );
    append_scalar_value(&mut key, value)?;
    Ok(key)
}

pub(super) fn unique_scalar_index_reservation_key(
    collection: &str,
    index_name: &str,
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let mut key = data_scoped_key(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"i", index_name.as_bytes()],
    );
    for value in values {
        append_scalar_value(&mut key, value)?;
    }
    Ok(key)
}

pub(super) fn unique_index_reservation_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_UNIQUE_RESERVATION, collection, &[b"i"])
}

pub(super) fn unique_scalar_index_reservation_prefix(
    collection: &str,
    index_name: &str,
) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_UNIQUE_RESERVATION,
        collection,
        &[b"i", index_name.as_bytes()],
    )
}

pub(super) fn column_batch_metadata_key(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_key(
        FAMILY_COLUMN_BATCH,
        collection,
        &[index_name.as_bytes(), b"m"],
    )
}

pub(super) fn column_batch_segment_key(
    collection: &str,
    index_name: &str,
    segment_id: u64,
) -> Vec<u8> {
    let encoded_segment = LexKey::encode_u64(segment_id);
    data_scoped_key(
        FAMILY_COLUMN_BATCH,
        collection,
        &[index_name.as_bytes(), b"s", encoded_segment.as_bytes()],
    )
}

pub(super) fn column_batch_index_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_COLUMN_BATCH, collection, &[index_name.as_bytes()])
}

pub(super) fn column_batch_collection_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_COLUMN_BATCH, collection, &[])
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
    scoped_key(FAMILY_VIEW, name, &[])
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
    scoped_key(FAMILY_SEQUENCE, name, &[])
}

pub(super) fn sequence_prefix() -> Vec<u8> {
    prefix(FAMILY_SEQUENCE, &[])
}

pub(super) fn constraints_key(collection: &str) -> Vec<u8> {
    scoped_key(FAMILY_CONSTRAINTS, collection, &[])
}

pub(super) fn namespace_key(namespace: &str) -> Vec<u8> {
    scoped_key(FAMILY_NAMESPACE, namespace, &[])
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
    data_scoped_prefix(FAMILY_ROW, collection, &[])
}

pub(super) fn row_key(collection: &str, id: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_ROW, collection, &[id.as_bytes()])
}

pub(super) fn doc_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_LEGACY_DOC, collection, &[])
}

pub(super) fn doc_key(collection: &str, id: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_LEGACY_DOC, collection, &[id.as_bytes()])
}

pub(super) fn cardinality_key(collection: &str) -> Vec<u8> {
    scoped_key(FAMILY_CARDINALITY, collection, &[])
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
    scoped_key(FAMILY_COLLECTION_METADATA, name, &[])
}

pub(super) fn column_store_collection_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_COLUMN_STORE, collection, &[])
}

pub(super) fn column_store_row_prefix(collection: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_COLUMN_STORE, collection, &[b"r"])
}

pub(super) fn column_store_row_key(collection: &str, id: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_COLUMN_STORE, collection, &[b"r", id.as_bytes()])
}

pub(super) fn column_store_deleted_key(collection: &str, id: &str) -> Vec<u8> {
    data_scoped_key(FAMILY_COLUMN_STORE, collection, &[b"x", id.as_bytes()])
}

pub(super) fn column_store_field_key(collection: &str, field: &str, id: &str) -> Vec<u8> {
    data_scoped_key(
        FAMILY_COLUMN_STORE,
        collection,
        &[b"f", field.as_bytes(), id.as_bytes()],
    )
}

pub(super) fn rollup_prefix() -> Vec<u8> {
    prefix(FAMILY_ROLLUP, &[])
}

pub(super) fn rollup_key(name: &str) -> Vec<u8> {
    let normalized = name.trim().to_ascii_lowercase();
    scoped_key(FAMILY_ROLLUP, &normalized, &[])
}

pub(super) fn retention_prefix() -> Vec<u8> {
    prefix(FAMILY_RETENTION, &[])
}

pub(super) fn retention_key(name: &str) -> Vec<u8> {
    let normalized = name.trim().to_ascii_lowercase();
    scoped_key(FAMILY_RETENTION, &normalized, &[])
}

pub(super) fn utf8_suffix_after_prefix(key: &[u8], prefix: &[u8]) -> Option<String> {
    let suffix = key.strip_prefix(prefix)?;
    let parts = suffix
        .split(|byte| *byte == LexKey::SEPARATOR)
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(parts.join("."))
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
    partition_key: &str,
    bucket_start_seconds: i64,
    id: &str,
) -> Vec<u8> {
    let mut key = time_series_index_partition_prefix(collection, index_name, partition_key);
    key.push(LexKey::SEPARATOR);
    let mut encoder = Encoder::with_capacity(16);
    encoder.encode_i64_into(bucket_start_seconds);
    key.extend_from_slice(encoder.as_slice());
    key.push(LexKey::SEPARATOR);
    append_terminated_component(&mut key, id.as_bytes());
    key
}

pub(super) fn time_series_index_partition_prefix(
    collection: &str,
    index_name: &str,
    partition_key: &str,
) -> Vec<u8> {
    data_scoped_key(
        FAMILY_TIME_SERIES_INDEX,
        collection,
        &[index_name.as_bytes(), b"d", partition_key.as_bytes()],
    )
}

pub(super) fn time_series_index_bucket_bound_key(
    collection: &str,
    index_name: &str,
    partition_key: &str,
    bucket_start_seconds: i64,
) -> Vec<u8> {
    let mut key = time_series_index_partition_prefix(collection, index_name, partition_key);
    key.push(LexKey::SEPARATOR);
    let mut encoder = Encoder::with_capacity(16);
    encoder.encode_i64_into(bucket_start_seconds);
    key.extend_from_slice(encoder.as_slice());
    key
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

pub(super) type ScalarIndexQueryBounds = (Option<Vec<u8>>, Option<Vec<u8>>);

pub(super) fn scalar_index_query_bounds(
    seek_prefix: &[u8],
    lower_bound: Option<&super::ScalarIndexBound>,
    upper_bound: Option<&super::ScalarIndexBound>,
) -> Result<ScalarIndexQueryBounds, CassieError> {
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

fn scoped_key(family: &[u8], scoped_name: &str, extra_components: &[&[u8]]) -> Vec<u8> {
    encode_scoped(family, scoped_name, extra_components, false)
}

fn scoped_prefix(family: &[u8], scoped_name: &str, extra_components: &[&[u8]]) -> Vec<u8> {
    encode_scoped(family, scoped_name, extra_components, true)
}

fn data_scoped_key(family: &[u8], scoped_name: &str, extra_components: &[&[u8]]) -> Vec<u8> {
    encode_data_scoped(family, scoped_name, extra_components, false)
}

fn data_scoped_prefix(family: &[u8], scoped_name: &str, extra_components: &[&[u8]]) -> Vec<u8> {
    encode_data_scoped(family, scoped_name, extra_components, true)
}

fn encode_scoped(
    family: &[u8],
    scoped_name: &str,
    extra_components: &[&[u8]],
    trailing_separator: bool,
) -> Vec<u8> {
    let owned_components = scoped_components(scoped_name);
    let mut components = owned_components
        .iter()
        .map(String::as_bytes)
        .collect::<Vec<_>>();
    components.extend_from_slice(extra_components);
    encode(family, &components, trailing_separator)
}

fn encode_data_scoped(
    family: &[u8],
    scoped_name: &str,
    extra_components: &[&[u8]],
    trailing_separator: bool,
) -> Vec<u8> {
    let owned_components = data_scoped_components(scoped_name);
    let mut components = owned_components
        .iter()
        .map(String::as_bytes)
        .collect::<Vec<_>>();
    components.extend_from_slice(extra_components);
    encode(family, &components, trailing_separator)
}

fn scoped_components(raw: &str) -> Vec<String> {
    split_identifier_path(raw)
        .unwrap_or_else(|_| vec![raw.trim().to_string()])
        .into_iter()
        .filter(|component| !component.is_empty())
        .collect()
}

/// Convert a canonical `database.schema.relation` name to the local key scope
/// used inside that database's physical family. Schema/catalog keys continue to
/// use `scoped_components` so the global catalog remains database-qualified.
fn data_scoped_components(raw: &str) -> Vec<String> {
    let components = scoped_components(raw);
    if components.len() >= 3 {
        components[1..].to_vec()
    } else {
        components
    }
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
mod key_encoding_tests;
