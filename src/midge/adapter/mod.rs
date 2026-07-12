use std::cell::Cell;
use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::OnceLock;
use std::time::Instant;

use cntryl_midge::{ColumnFamilyHandle, Engine, Query, TransactionMode, WriteOptions};
use uuid::Uuid;

use crate::app::CassieError;
use crate::catalog::{
    payload_contains_index_membership, payload_contains_vector_membership,
    CollectionCardinalityStats, CollectionMeta, CollectionStorageMode, ColumnBatchCodecMeta,
    ColumnBatchColumn, ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchPayload,
    ColumnBatchRow, ColumnBatchSegmentMeta, ColumnBatchValueRun, DatabaseMeta,
    FieldCardinalityStats, FieldConstraint, FieldHeavyHitter, FieldHistogramBucket, IndexKind,
    IndexMeta, NamespaceMeta, OperationalAssignmentMeta, ProjectionMeta, RetentionPolicyMeta,
    RoleMeta, RollupMeta,
};
use crate::embeddings::{NormalizedVectorRecord, VectorIndexRecord, VectorIndexState};
use crate::midge::row_blob::{
    decode_projected_row, decode_projected_row_matching_with_aliases,
    decode_projected_row_with_aliases, decode_row, encode_row, RowSchema,
};
use crate::types::{DataType, FieldSchema, Schema, Value, Vector};
use crate::vector::normalize as normalize_vector;

mod core;
mod raw_ops;
mod transactions;

pub use core::Midge;

static DOCUMENT_WRITE_FAILPOINT: AtomicU8 = AtomicU8::new(0);
static DOCUMENT_WRITE_FAILPOINT_TEST_GUARD: OnceLock<parking_lot::Mutex<()>> = OnceLock::new();
static COLUMN_BATCH_MAINTENANCE_FAILPOINT: AtomicBool = AtomicBool::new(false);
static PROJECTION_HASH_MAINTENANCE_FAILPOINT: AtomicBool = AtomicBool::new(false);
static ROLLUP_MAINTENANCE_FAILPOINT: AtomicBool = AtomicBool::new(false);
static COLLECTION_DROP_FAILPOINT: AtomicBool = AtomicBool::new(false);
static INDEX_PUBLICATION_FAILPOINT: AtomicBool = AtomicBool::new(false);
static INDEX_DROP_FAILPOINT: AtomicBool = AtomicBool::new(false);
static COLLECTION_RENAME_FAILPOINT: AtomicBool = AtomicBool::new(false);
static FIELD_ADD_FAILPOINT: AtomicBool = AtomicBool::new(false);
static FIELD_RENAME_FAILPOINT: AtomicBool = AtomicBool::new(false);
static FIELD_DROP_FAILPOINT: AtomicBool = AtomicBool::new(false);

thread_local! {
    static DOCUMENT_WRITE_CONFLICTS_REMAINING: Cell<u8> = const { Cell::new(0) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum DocumentWriteFailurePoint {
    Row,
    ScalarIndex,
    TimeSeriesIndex,
    GraphAdjacency,
    NormalizedVector,
    VectorState,
}

impl DocumentWriteFailurePoint {
    const fn code(self) -> u8 {
        match self {
            Self::Row => 1,
            Self::ScalarIndex => 2,
            Self::TimeSeriesIndex => 3,
            Self::GraphAdjacency => 4,
            Self::NormalizedVector => 5,
            Self::VectorState => 6,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::ScalarIndex => "scalar-index",
            Self::TimeSeriesIndex => "time-series-index",
            Self::GraphAdjacency => "graph-adjacency",
            Self::NormalizedVector => "normalized-vector",
            Self::VectorState => "vector-state",
        }
    }
}

#[doc(hidden)]
pub fn set_document_write_failure_point(point: Option<DocumentWriteFailurePoint>) {
    DOCUMENT_WRITE_FAILPOINT.store(
        point.map_or(0, DocumentWriteFailurePoint::code),
        std::sync::atomic::Ordering::SeqCst,
    );
}

#[doc(hidden)]
pub fn document_write_failure_point_test_guard() -> parking_lot::MutexGuard<'static, ()> {
    DOCUMENT_WRITE_FAILPOINT_TEST_GUARD
        .get_or_init(|| parking_lot::Mutex::new(()))
        .lock()
}

#[doc(hidden)]
pub fn set_column_batch_maintenance_failure_point(enabled: bool) {
    COLUMN_BATCH_MAINTENANCE_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_column_batch_maintenance_failure_point() -> Result<(), CassieError> {
    if COLUMN_BATCH_MAINTENANCE_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure during column batch maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_projection_hash_maintenance_failure_point(enabled: bool) {
    PROJECTION_HASH_MAINTENANCE_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_projection_hash_maintenance_failure_point() -> Result<(), CassieError> {
    if PROJECTION_HASH_MAINTENANCE_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure during projection hash maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_rollup_maintenance_failure_point(enabled: bool) {
    ROLLUP_MAINTENANCE_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_rollup_maintenance_failure_point() -> Result<(), CassieError> {
    if ROLLUP_MAINTENANCE_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure during rollup maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_collection_drop_failure_point(enabled: bool) {
    COLLECTION_DROP_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_collection_drop_failure_point() -> Result<(), CassieError> {
    if COLLECTION_DROP_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure after collection drop schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_index_publication_failure_point(enabled: bool) {
    INDEX_PUBLICATION_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_index_publication_failure_point() -> Result<(), CassieError> {
    if INDEX_PUBLICATION_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure during index publication".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_index_drop_failure_point(enabled: bool) {
    INDEX_DROP_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_index_drop_failure_point() -> Result<(), CassieError> {
    if INDEX_DROP_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure during index drop cleanup".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_collection_rename_failure_point(enabled: bool) {
    COLLECTION_RENAME_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_collection_rename_failure_point() -> Result<(), CassieError> {
    if COLLECTION_RENAME_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure after collection rename schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_add_failure_point(enabled: bool) {
    FIELD_ADD_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_field_add_failure_point() -> Result<(), CassieError> {
    if FIELD_ADD_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure after field add schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_rename_failure_point(enabled: bool) {
    FIELD_RENAME_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_field_rename_failure_point() -> Result<(), CassieError> {
    if FIELD_RENAME_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure after field rename schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_drop_failure_point(enabled: bool) {
    FIELD_DROP_FAILPOINT.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn check_field_drop_failure_point() -> Result<(), CassieError> {
    if FIELD_DROP_FAILPOINT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CassieError::Execution(
            "injected test failure after field drop schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub(crate) fn check_document_write_failure_point(
    point: DocumentWriteFailurePoint,
) -> Result<(), CassieError> {
    let requested = DOCUMENT_WRITE_FAILPOINT
        .compare_exchange(
            point.code(),
            0,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .ok();
    if requested.is_none() {
        return Ok(());
    }

    Err(CassieError::Execution(format!(
        "injected test failure after {} mutation",
        point.label()
    )))
}

#[doc(hidden)]
pub fn set_document_write_conflicts_remaining(remaining: u8) {
    DOCUMENT_WRITE_CONFLICTS_REMAINING.with(|counter| counter.set(remaining));
}

#[doc(hidden)]
pub(crate) fn check_document_write_conflict_injection() -> Result<(), CassieError> {
    let injected = DOCUMENT_WRITE_CONFLICTS_REMAINING.with(|counter| {
        let remaining = counter.get();
        if remaining == 0 {
            return false;
        }
        counter.set(remaining.saturating_sub(1));
        true
    });
    if injected {
        return Err(CassieError::StorageRetryable(
            "midge write conflict: injected test conflict".to_string(),
        ));
    }

    Ok(())
}

mod capacity;
mod cardinality_stats;
mod column_batches;
mod column_store;
mod databases;
pub(crate) use column_store::{ColumnStoreScanRequest, OrderedColumnStoreScanRequest};
pub(super) use databases::DatabaseFamily;
pub(crate) use databases::StagedDatabaseFamily;
pub(crate) mod documents;
mod fresh_documents;
mod graphs;
mod key_encoding;
mod layout;
use layout::{
    allow_memory_fallback, FamilyScope, RawStorageEntry, DEFAULT_FAMILY_NAME, SCHEMA_FAMILY_NAME,
    TEMP_FAMILY_NAME,
};
pub use layout::{StorageFamily, StorageLayout};
mod index_publication;
mod maintenance;
mod metadata;
mod operational;
mod operator_feedback;
mod projections;
mod repair;
mod scalar_indexes;
pub(crate) use scalar_indexes::{ScalarIndexBound, ScalarIndexScanRequest};
mod scan_types;
mod schema_cleanup;
mod schema_ops;
mod sequences;
mod streaming_scans;
pub(crate) mod time_series_indexes;
mod vector_indexes;
mod verification;

pub(crate) use documents::{DocumentWriteBatchOptions, DocumentWriteOp, OrderedRowScanRequest};
pub(crate) use graphs::GraphEdgeRecord;
pub(crate) use scan_types::OrderedRowBound;
pub use scan_types::{
    ColumnBatchScanDecision, ColumnBatchScanFallbackReason, ColumnBatchScanFilter,
    ColumnBatchScanOp, ColumnBatchScanOutcome, ColumnBatchScanPredicate, DocumentRef,
    MidgeScanTimings, RowDecode, RowFilter,
};
pub use verification::{
    IntegrityCheckReport, RangeHashRecord, RootHashRecord, RowHashRecord, StoredHashState,
};

impl Midge {
    fn collection_schema_key(collection: &str) -> Vec<u8> {
        key_encoding::collection_schema_key(collection)
    }

    fn row_schema_key(collection: &str) -> Vec<u8> {
        key_encoding::row_schema_key(collection)
    }

    fn projection_key(collection: &str) -> Vec<u8> {
        key_encoding::projection_key(collection)
    }

    fn projection_prefix() -> Vec<u8> {
        key_encoding::projection_prefix()
    }

    fn projection_comparison_report_key(report_id: &str) -> Vec<u8> {
        key_encoding::projection_comparison_report_key(report_id)
    }

    fn projection_comparison_report_prefix() -> Vec<u8> {
        key_encoding::projection_comparison_report_prefix()
    }

    fn projection_consistency_report_key(report_id: &str) -> Vec<u8> {
        key_encoding::projection_consistency_report_key(report_id)
    }

    fn projection_consistency_report_prefix() -> Vec<u8> {
        key_encoding::projection_consistency_report_prefix()
    }

    fn projection_event_key(projection: &str, source_identity: &str, event_id: &str) -> Vec<u8> {
        key_encoding::projection_event_key(projection, source_identity, event_id)
    }

    fn projection_event_prefix(projection: &str) -> Vec<u8> {
        key_encoding::projection_event_prefix(projection)
    }

    fn operational_assignment_key(assignment_id: &str) -> Vec<u8> {
        key_encoding::operational_assignment_key(assignment_id)
    }

    fn operational_assignment_prefix() -> Vec<u8> {
        key_encoding::operational_assignment_prefix()
    }

    fn row_hash_key(collection: &str, row_id: &str) -> Vec<u8> {
        key_encoding::row_hash_key(collection, row_id)
    }

    fn row_hash_prefix(collection: &str) -> Vec<u8> {
        key_encoding::row_hash_prefix(collection)
    }

    fn range_hash_key(collection: &str, range_id: u64) -> Vec<u8> {
        key_encoding::range_hash_key(collection, range_id)
    }

    fn range_hash_prefix(collection: &str) -> Vec<u8> {
        key_encoding::range_hash_prefix(collection)
    }

    fn root_hash_key(collection: &str) -> Vec<u8> {
        key_encoding::root_hash_key(collection)
    }

    fn schema_collection_prefix() -> Vec<u8> {
        key_encoding::schema_collection_prefix()
    }

    fn vector_index_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::vector_index_key(collection, field)
    }

    fn vector_index_prefix() -> Vec<u8> {
        key_encoding::vector_index_prefix()
    }

    fn vector_index_state_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::vector_index_state_key(collection, field)
    }

    fn vector_index_state_prefix(collection: &str) -> Vec<u8> {
        key_encoding::vector_index_state_prefix(collection)
    }

    fn data_epoch_key() -> Vec<u8> {
        key_encoding::data_epoch_key()
    }

    fn collection_generation_key(collection: &str) -> Vec<u8> {
        key_encoding::collection_generation_key(collection)
    }

    fn maintenance_debt_key(collection: &str, artifact: &str) -> Vec<u8> {
        key_encoding::maintenance_debt_key(collection, artifact)
    }

    fn maintenance_debt_prefix() -> Vec<u8> {
        key_encoding::maintenance_debt_prefix()
    }

    fn unique_constraint_reservation_field_prefix(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::unique_constraint_reservation_field_prefix(collection, field)
    }

    fn unique_scalar_index_reservation_prefix(collection: &str, index_name: &str) -> Vec<u8> {
        key_encoding::unique_scalar_index_reservation_prefix(collection, index_name)
    }

    fn index_publication_key(collection: &str, index: &str) -> Vec<u8> {
        key_encoding::index_publication_key(collection, index)
    }

    fn index_publication_prefix() -> Vec<u8> {
        key_encoding::index_publication_prefix()
    }

    fn vector_index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::vector_index_collection_prefix(collection)
    }

    fn normalized_vector_key(collection: &str, field: &str, id: &str) -> Vec<u8> {
        key_encoding::normalized_vector_key(collection, field, id)
    }

    fn normalized_vector_prefix(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::normalized_vector_prefix(collection, field)
    }

    fn normalized_vector_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::normalized_vector_collection_prefix(collection)
    }

    fn index_key(collection: &str, name: &str) -> Vec<u8> {
        key_encoding::index_key(collection, name)
    }

    fn rollup_prefix() -> Vec<u8> {
        key_encoding::rollup_prefix()
    }

    fn rollup_key(name: &str) -> Vec<u8> {
        key_encoding::rollup_key(name)
    }

    fn retention_prefix() -> Vec<u8> {
        key_encoding::retention_prefix()
    }

    fn retention_key(name: &str) -> Vec<u8> {
        key_encoding::retention_key(name)
    }

    fn graph_prefix() -> Vec<u8> {
        key_encoding::graph_prefix()
    }

    fn graph_key(name: &str) -> Vec<u8> {
        key_encoding::graph_key(name)
    }

    fn graph_outbound_prefix(graph: &str, source_type: &str, source_id: &str) -> Vec<u8> {
        key_encoding::graph_outbound_prefix(graph, source_type, source_id)
    }

    fn graph_inbound_prefix(graph: &str, target_type: &str, target_id: &str) -> Vec<u8> {
        key_encoding::graph_inbound_prefix(graph, target_type, target_id)
    }

    fn graph_outbound_edge_key(
        graph: &str,
        source_type: &str,
        source_id: &str,
        edge_type: &str,
        target_type: &str,
        target_id: &str,
        edge_id: &str,
    ) -> Vec<u8> {
        key_encoding::graph_outbound_edge_key(
            graph,
            source_type,
            source_id,
            edge_type,
            target_type,
            target_id,
            edge_id,
        )
    }

    fn graph_inbound_edge_key(
        graph: &str,
        target_type: &str,
        target_id: &str,
        edge_type: &str,
        source_type: &str,
        source_id: &str,
        edge_id: &str,
    ) -> Vec<u8> {
        key_encoding::graph_inbound_edge_key(
            graph,
            target_type,
            target_id,
            edge_type,
            source_type,
            source_id,
            edge_id,
        )
    }

    fn index_prefix() -> Vec<u8> {
        key_encoding::index_prefix()
    }

    fn index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::index_collection_prefix(collection)
    }

    fn scalar_index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::scalar_index_collection_prefix(collection)
    }

    fn scalar_index_data_prefix(collection: &str, index_name: &str) -> Vec<u8> {
        key_encoding::scalar_index_data_prefix(collection, index_name)
    }

    fn time_series_index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::time_series_index_collection_prefix(collection)
    }

    fn time_series_index_data_prefix(collection: &str, index_name: &str) -> Vec<u8> {
        key_encoding::time_series_index_data_prefix(collection, index_name)
    }

    fn column_batch_metadata_key(collection: &str, index_name: &str) -> Vec<u8> {
        key_encoding::column_batch_metadata_key(collection, index_name)
    }

    fn column_batch_segment_key(collection: &str, index_name: &str, segment_id: u64) -> Vec<u8> {
        key_encoding::column_batch_segment_key(collection, index_name, segment_id)
    }

    fn column_batch_index_prefix(collection: &str, index_name: &str) -> Vec<u8> {
        key_encoding::column_batch_index_prefix(collection, index_name)
    }

    fn column_batch_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::column_batch_collection_prefix(collection)
    }

    fn function_key(name: &str) -> Vec<u8> {
        key_encoding::function_key(name)
    }

    fn function_prefix() -> Vec<u8> {
        key_encoding::function_prefix()
    }

    fn procedure_key(name: &str) -> Vec<u8> {
        key_encoding::procedure_key(name)
    }

    fn procedure_prefix() -> Vec<u8> {
        key_encoding::procedure_prefix()
    }

    fn view_key(name: &str) -> Vec<u8> {
        key_encoding::view_key(name)
    }

    fn view_prefix() -> Vec<u8> {
        key_encoding::view_prefix()
    }

    fn role_key(name: &str) -> Vec<u8> {
        key_encoding::role_key(name)
    }

    fn role_prefix() -> Vec<u8> {
        key_encoding::role_prefix()
    }

    fn constraints_key(collection: &str) -> Vec<u8> {
        key_encoding::constraints_key(collection)
    }

    fn database_key(name: &str) -> Vec<u8> {
        key_encoding::database_key(name)
    }

    fn database_prefix() -> Vec<u8> {
        key_encoding::database_prefix()
    }

    fn databases_key() -> Vec<u8> {
        key_encoding::databases_key()
    }

    fn namespace_key(namespace: &str) -> Vec<u8> {
        key_encoding::namespace_key(namespace)
    }

    fn namespace_prefix() -> Vec<u8> {
        key_encoding::namespace_prefix()
    }

    fn namespaces_key() -> Vec<u8> {
        key_encoding::namespaces_key()
    }

    fn schema_epoch_key() -> Vec<u8> {
        key_encoding::schema_epoch_key()
    }

    fn schema_cleanup_key(cleanup_id: &str) -> Vec<u8> {
        key_encoding::schema_cleanup_key(cleanup_id)
    }

    fn schema_cleanup_prefix() -> Vec<u8> {
        key_encoding::schema_cleanup_prefix()
    }

    fn schema_operation_key(current: &str, next: &str) -> Vec<u8> {
        key_encoding::schema_operation_key(current, next)
    }

    fn schema_operation_prefix() -> Vec<u8> {
        key_encoding::schema_operation_prefix()
    }

    fn field_rename_operation_key(collection: &str, current: &str, next: &str) -> Vec<u8> {
        key_encoding::field_rename_operation_key(collection, current, next)
    }

    fn field_add_operation_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::field_add_operation_key(collection, field)
    }

    fn field_add_operation_prefix() -> Vec<u8> {
        key_encoding::field_add_operation_prefix()
    }

    fn field_rename_operation_prefix() -> Vec<u8> {
        key_encoding::field_rename_operation_prefix()
    }

    fn field_drop_operation_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::field_drop_operation_key(collection, field)
    }

    fn field_drop_operation_prefix() -> Vec<u8> {
        key_encoding::field_drop_operation_prefix()
    }

    fn collections_key() -> Vec<u8> {
        key_encoding::collections_key()
    }

    fn row_prefix(collection: &str) -> Vec<u8> {
        key_encoding::row_prefix(collection)
    }

    fn row_key(collection: &str, id: &str) -> Vec<u8> {
        key_encoding::row_key(collection, id)
    }

    fn doc_prefix(collection: &str) -> Vec<u8> {
        key_encoding::doc_prefix(collection)
    }

    fn doc_key(collection: &str, id: &str) -> Vec<u8> {
        key_encoding::doc_key(collection, id)
    }

    fn load_schema_epoch_from_tx(tx: &cntryl_midge::Transaction) -> Result<u64, CassieError> {
        let Some(raw) = tx
            .get(&Self::schema_epoch_key())
            .map_err(CassieError::from)?
        else {
            return Ok(0);
        };

        serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid schema epoch: {error}")))
    }

    fn save_schema_epoch_to_tx(
        tx: &mut cntryl_midge::Transaction,
        schema_epoch: u64,
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(&schema_epoch)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::schema_epoch_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn schema_epoch(&self) -> Result<u64, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_schema_epoch_from_tx(&tx)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn bump_schema_epoch(&self) -> Result<u64, CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let next = Self::load_schema_epoch_from_tx(&tx)?.saturating_add(1);
        Self::save_schema_epoch_to_tx(&mut tx, next)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(next)
    }

    fn load_collections(tx: &cntryl_midge::Transaction) -> Result<Vec<String>, CassieError> {
        let Some(raw) = tx
            .get(&Self::collections_key())
            .map_err(CassieError::from)?
        else {
            return Ok(Vec::new());
        };
        let parsed: Vec<String> =
            serde_json::from_slice(&raw).map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    fn load_databases(tx: &cntryl_midge::Transaction) -> Result<Vec<String>, CassieError> {
        let Some(raw) = tx.get(&Self::databases_key()).map_err(CassieError::from)? else {
            return Ok(Vec::new());
        };
        let parsed: Vec<String> =
            serde_json::from_slice(&raw).map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    fn load_namespaces(tx: &cntryl_midge::Transaction) -> Result<Vec<String>, CassieError> {
        let Some(raw) = tx.get(&Self::namespaces_key()).map_err(CassieError::from)? else {
            return Ok(Vec::new());
        };
        let parsed: Vec<String> =
            serde_json::from_slice(&raw).map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    fn save_collections(
        tx: &mut cntryl_midge::Transaction,
        collections: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(collections)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::collections_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn save_databases(
        tx: &mut cntryl_midge::Transaction,
        databases: &[String],
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(databases).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::databases_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn save_namespaces(
        tx: &mut cntryl_midge::Transaction,
        namespaces: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(namespaces)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::namespaces_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn load_row_schema_from_tx(
        tx: &cntryl_midge::Transaction,
        collection: &str,
    ) -> Result<Option<RowSchema>, CassieError> {
        let raw = tx
            .get(&Self::row_schema_key(collection))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        let mut row_schema: RowSchema = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid row schema for '{collection}': {error}"))
        })?;
        for field in &mut row_schema.fields {
            if field.normalized_name.is_empty() {
                field.normalized_name = field.name.to_ascii_lowercase();
            }
        }
        Ok(Some(row_schema))
    }

    fn save_row_schema_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        row_schema: &RowSchema,
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(row_schema)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::row_schema_key(collection), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn load_projection_metadata_from_tx(
        tx: &cntryl_midge::Transaction,
        collection: &str,
    ) -> Result<Option<ProjectionMeta>, CassieError> {
        let Some(raw) = tx
            .get(&Self::projection_key(collection))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw).map(Some).map_err(|error| {
            CassieError::Parse(format!(
                "invalid projection metadata for '{collection}': {error}"
            ))
        })
    }

    fn save_projection_metadata_to_tx(
        tx: &mut cntryl_midge::Transaction,
        metadata: &ProjectionMeta,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::projection_key(&metadata.collection), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn cardinality_key(collection: &str) -> Vec<u8> {
        key_encoding::cardinality_key(collection)
    }

    fn cardinality_prefix() -> Vec<u8> {
        key_encoding::cardinality_prefix()
    }

    fn runtime_feedback_key(key: &crate::runtime::RuntimeFeedbackKey) -> Vec<u8> {
        key_encoding::runtime_feedback_key(crate::runtime::stable_fingerprint(key))
    }

    fn runtime_feedback_prefix() -> Vec<u8> {
        key_encoding::runtime_feedback_prefix()
    }

    fn load_cardinality_stats_from_tx(
        tx: &cntryl_midge::Transaction,
        collection: &str,
    ) -> Result<Option<CollectionCardinalityStats>, CassieError> {
        let Some(raw) = tx
            .get(Self::cardinality_key(collection).as_slice())
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw).map(Some).map_err(|error| {
            CassieError::Parse(format!(
                "invalid cardinality stats for '{collection}': {error}"
            ))
        })
    }

    fn save_cardinality_stats_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        stats: &CollectionCardinalityStats,
    ) -> Result<(), CassieError> {
        tx.put(
            Self::cardinality_key(collection),
            serde_json::to_vec(stats).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        Ok(())
    }

    fn update_projection_schema_version_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        schema_version: u32,
    ) -> Result<(), CassieError> {
        let mut metadata = Self::load_projection_metadata_from_tx(tx, collection)?
            .unwrap_or_else(|| ProjectionMeta::new(collection, schema_version));
        metadata.schema_version = schema_version;
        Self::save_projection_metadata_to_tx(tx, &metadata)
    }

    fn row_schema(&self, collection: &str) -> Result<RowSchema, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_schema_readonly_tx()?;
        if let Some(row_schema) = Self::load_row_schema_from_tx(&tx, &collection)? {
            return Ok(row_schema);
        }

        let raw = tx
            .get(&Self::collection_schema_key(&collection))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.clone()))?;
        let schema: Schema = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid schema for '{collection}': {error}"))
        })?;
        Ok(RowSchema::from_schema(&schema))
    }
}

impl From<&Value> for Vector {
    fn from(value: &Value) -> Self {
        match value {
            Value::Vector(v) => v.clone(),
            _ => Vector::new(Vec::new()),
        }
    }
}

#[must_use]
pub fn vector_from_json(value: &serde_json::Value) -> Option<Vector> {
    let arr = value.as_array()?;
    let mut nums = Vec::with_capacity(arr.len());
    for n in arr {
        nums.push(n.as_f64()?.to_string().parse::<f32>().ok()?);
    }
    Some(Vector::new(nums))
}
