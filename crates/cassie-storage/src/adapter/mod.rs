use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::path::Path;
use std::time::Instant;

use cntryl_midge::{ColumnFamilyHandle, Engine, Query, TransactionMode, WriteOptions};
use uuid::Uuid;

pub mod error;
pub(crate) use error::{
    decode_projected_row_matching_with_aliases_and_hooks,
    decode_projected_row_with_aliases_and_hooks, decode_projected_row_with_hooks,
    decode_row_with_hooks,
};
pub use error::{
    CatalogObjectKind, RetryableStorageError, StorageError, StorageRetryKind,
    TransactionErrorKind,
};
pub(crate) use error::StorageError as CassieError;
use crate::catalog::{
    payload_contains_index_membership, payload_contains_vector_membership,
    CollectionCardinalityStats, CollectionMeta, CollectionStorageMode, ColumnBatchChunkMeta,
    ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchRow, ColumnBatchSegmentMeta,
    DatabaseMeta, FieldCardinalityStats, FieldConstraint, FieldHeavyHitter, FieldHistogramBucket,
    IndexKind, IndexMeta, NamespaceMeta, OperationalAssignmentMeta, ProjectionMeta,
    RetentionPolicyMeta, RoleMeta, RollupMeta,
};
use crate::embeddings::{NormalizedVectorRecord, VectorIndexRecord, VectorIndexState};
use crate::row_blob::{
    encode_row, RowSchema,
};
use cassie_core::types::{DataType, FieldSchema, Schema};
use crate::vector::normalize as normalize_vector;

pub use crate::column_batch_format_v1::NumericValues;
mod collection_gates;
mod commit_source;
mod core;
pub mod fulltext_retrieval;
#[cfg(feature = "replay-benchmark-observation")]
mod native_metrics;
#[cfg(feature = "replay-benchmark-observation")]
pub use native_metrics::{
    observe_native_commits, NativeCommitObservation, NativeDurabilityMetrics,
};
mod object_comments;
mod publication;
mod role_object_grants;
pub use publication::{AdapterCommitError, RetainedRowHistory};
mod raw_ops;
#[doc(hidden)]
pub use publication::NativePublicationControlScope;
mod transactions;
pub use transactions::{
    AdapterMutationError, AdapterTransaction, CheckpointStorageSnapshot, CopyStorageSnapshot,
    IndexCleanupCommitGuard, IndexCleanupGateNames, PreparedSequenceCursorMetadata,
    RetainedRowFence, StagedCheckpointChange, StorageOverlay,
};

#[doc(hidden)]
pub use crate::column_batch_format_v1::{
    decode_column_batch_manifest_for_test, encode_column_batch_manifest_for_test,
};
pub use core::{Midge, StorageValueHooks};
pub use repair::set_projection_report_deletion_failure_point;

thread_local! {
    static COLUMN_BATCH_MAINTENANCE_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static PROJECTION_HASH_MAINTENANCE_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static PROJECTION_OUTPUT_FAILURE_POINT: Cell<Option<ProjectionOutputFailurePoint>> = const { Cell::new(None) };
    static PROJECTION_METADATA_PERSISTENCE_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static PROJECTION_REPLAY_METADATA_COMMIT_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static PROJECTION_REPLAY_POST_COMMIT_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static ROLLUP_MAINTENANCE_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static COLLECTION_DROP_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static INDEX_PUBLICATION_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static INDEX_DROP_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static UNIQUE_CONSTRAINT_CLEANUP_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static COLLECTION_RENAME_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static VIEW_RENAME_PUBLICATION_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static NAMESPACE_METADATA_RENAME_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static FIELD_ADD_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static FIELD_ADD_BACKFILL_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static FIELD_RENAME_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static FIELD_DROP_FAILPOINT: Cell<bool> = const { Cell::new(false) };
    static OPERATOR_FEEDBACK_PERSISTENCE_FAILPOINT: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionOutputFailurePoint {
    AfterRowBatches,
    AfterHashPublication,
}

#[doc(hidden)]
pub fn set_projection_output_failure_point(failure_point: Option<ProjectionOutputFailurePoint>) {
    PROJECTION_OUTPUT_FAILURE_POINT.set(failure_point);
}

pub(crate) fn check_projection_output_failure_point(
    expected: ProjectionOutputFailurePoint,
) -> Result<(), CassieError> {
    if PROJECTION_OUTPUT_FAILURE_POINT.get() == Some(expected) {
        PROJECTION_OUTPUT_FAILURE_POINT.set(None);
        let stage = match expected {
            ProjectionOutputFailurePoint::AfterRowBatches => "after row batches",
            ProjectionOutputFailurePoint::AfterHashPublication => "after hash publication",
        };
        return Err(CassieError::Execution(format!(
            "injected projection output failure {stage}"
        )));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_projection_metadata_persistence_failure_point(enabled: bool) {
    PROJECTION_METADATA_PERSISTENCE_FAILPOINT.set(enabled);
}

pub(crate) fn check_projection_metadata_persistence_failure_point() -> Result<(), CassieError> {
    if PROJECTION_METADATA_PERSISTENCE_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected projection metadata persistence failure".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_projection_replay_metadata_commit_failure_point(enabled: bool) {
    PROJECTION_REPLAY_METADATA_COMMIT_FAILPOINT.set(enabled);
}

pub(crate) fn check_projection_replay_metadata_commit_failure_point() -> Result<(), CassieError> {
    if PROJECTION_REPLAY_METADATA_COMMIT_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected projection replay metadata commit failure".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_projection_replay_post_commit_failure_point(enabled: bool) {
    PROJECTION_REPLAY_POST_COMMIT_FAILPOINT.set(enabled);
}

pub fn check_projection_replay_post_commit_failure_point() -> Result<(), CassieError> {
    if PROJECTION_REPLAY_POST_COMMIT_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected projection replay failure after metadata commit".to_string(),
        ));
    }
    Ok(())
}

#[derive(Default)]
struct ColumnBatchOperationalMetrics {
    codec_choices: BTreeMap<String, u64>,
    full_rebuilds: u64,
    segment_rewrites: u64,
    segment_splits: u64,
    compactions: u64,
    maintenance_bytes: u64,
    maintenance_source_rows: u64,
    orphan_revisions_cleaned: u64,
    write_amplification_buckets: [u64; 3],
}

#[doc(hidden)]
pub fn set_column_batch_maintenance_failure_point(enabled: bool) {
    COLUMN_BATCH_MAINTENANCE_FAILPOINT.set(enabled);
}

pub(crate) fn check_column_batch_maintenance_failure_point() -> Result<(), CassieError> {
    if COLUMN_BATCH_MAINTENANCE_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure during column batch maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_projection_hash_maintenance_failure_point(enabled: bool) {
    PROJECTION_HASH_MAINTENANCE_FAILPOINT.set(enabled);
}

pub(crate) fn check_projection_hash_maintenance_failure_point() -> Result<(), CassieError> {
    if PROJECTION_HASH_MAINTENANCE_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure during projection hash maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_rollup_maintenance_failure_point(enabled: bool) {
    ROLLUP_MAINTENANCE_FAILPOINT.set(enabled);
}

pub fn check_rollup_maintenance_failure_point() -> Result<(), CassieError> {
    if ROLLUP_MAINTENANCE_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure during rollup maintenance".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_operator_feedback_persistence_failure_point(enabled: bool) {
    OPERATOR_FEEDBACK_PERSISTENCE_FAILPOINT.set(enabled);
}

pub(crate) fn check_operator_feedback_persistence_failure_point() -> Result<(), CassieError> {
    if OPERATOR_FEEDBACK_PERSISTENCE_FAILPOINT.replace(false) {
        return Err(CassieError::Storage(
            "injected test failure during operator feedback persistence".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_collection_drop_failure_point(enabled: bool) {
    COLLECTION_DROP_FAILPOINT.set(enabled);
}

pub(crate) fn check_collection_drop_failure_point() -> Result<(), CassieError> {
    if COLLECTION_DROP_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure after collection drop schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_index_publication_failure_point(enabled: bool) {
    INDEX_PUBLICATION_FAILPOINT.set(enabled);
}

pub(crate) fn check_index_publication_failure_point() -> Result<(), CassieError> {
    if INDEX_PUBLICATION_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure during index publication".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_index_drop_failure_point(enabled: bool) {
    INDEX_DROP_FAILPOINT.set(enabled);
}

pub(crate) fn check_index_drop_failure_point() -> Result<(), CassieError> {
    if INDEX_DROP_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure during index drop cleanup".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_unique_constraint_cleanup_failure_point(enabled: bool) {
    UNIQUE_CONSTRAINT_CLEANUP_FAILPOINT.set(enabled);
}

pub(crate) fn check_unique_constraint_cleanup_failure_point() -> Result<(), CassieError> {
    if UNIQUE_CONSTRAINT_CLEANUP_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure during unique constraint cleanup".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_collection_rename_failure_point(enabled: bool) {
    COLLECTION_RENAME_FAILPOINT.set(enabled);
}

pub(crate) fn check_collection_rename_failure_point() -> Result<(), CassieError> {
    if COLLECTION_RENAME_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure after collection rename schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_view_rename_publication_failure_point(enabled: bool) {
    VIEW_RENAME_PUBLICATION_FAILPOINT.set(enabled);
}

pub(crate) fn check_view_rename_publication_failure_point() -> Result<(), CassieError> {
    if VIEW_RENAME_PUBLICATION_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure before view rename publication".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_namespace_metadata_rename_failure_point(enabled: bool) {
    NAMESPACE_METADATA_RENAME_FAILPOINT.set(enabled);
}

pub(crate) fn check_namespace_metadata_rename_failure_point() -> Result<(), CassieError> {
    if NAMESPACE_METADATA_RENAME_FAILPOINT.replace(false) {
        return Err(CassieError::diagnostic(
            "XX000",
            "injected test failure before namespace metadata commit",
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_add_failure_point(enabled: bool) {
    FIELD_ADD_FAILPOINT.set(enabled);
}

pub(crate) fn check_field_add_failure_point() -> Result<(), CassieError> {
    if FIELD_ADD_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure after field add schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_add_backfill_failure_point(enabled: bool) {
    FIELD_ADD_BACKFILL_FAILPOINT.set(enabled);
}

pub(crate) fn check_field_add_backfill_failure_point() -> Result<(), CassieError> {
    if FIELD_ADD_BACKFILL_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure after field-add default backfill batch".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_rename_failure_point(enabled: bool) {
    FIELD_RENAME_FAILPOINT.set(enabled);
}

pub(crate) fn check_field_rename_failure_point() -> Result<(), CassieError> {
    if FIELD_RENAME_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure after field rename schema commit".to_string(),
        ));
    }
    Ok(())
}

#[doc(hidden)]
pub fn set_field_drop_failure_point(enabled: bool) {
    FIELD_DROP_FAILPOINT.set(enabled);
}

pub(crate) fn check_field_drop_failure_point() -> Result<(), CassieError> {
    if FIELD_DROP_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected test failure after field drop schema commit".to_string(),
        ));
    }
    Ok(())
}

mod capacity;
mod cardinality_stats;
mod column_batches;
mod vector_values;
pub use vector_values::vector_from_json;
mod column_store;
#[cfg(test)]
#[path = "column_store_tests.rs"]
mod column_store_tests;
mod database_catalog_rewrite;
mod databases;
pub mod graph_catalog_entries;
mod native_memory;
pub use native_memory::{NativeMemoryConfiguration, NativeMemoryDiagnostics, NativeMemoryRuntime};
mod storage_conversion;
#[doc(hidden)]
pub use storage_conversion::ConversionReport;
pub use column_store::field_selection::ColumnFieldReadSnapshot;
pub use column_store::field_selection::{ColumnFieldReadCounters, ColumnFieldSelection};
pub use column_store::ColumnPruningSnapshot;
pub use column_store::ColumnStoreScanRequest;
pub use column_store::ColumnVectorReadSnapshot;
#[cfg(feature = "storage-test-hooks")]
pub use column_store::vector_scan::test_support as column_vector_test_support;
pub use column_store::{
    identity_lanes, ColumnPartitionReader, ColumnVectorBatch, ColumnVectorOutput,
    ColumnVectorReadCounters, ColumnVectorReader, COLUMN_VECTOR_BATCH_ROWS,
};
pub use column_store::{ColumnPruning, ColumnPruningState};
pub use database_catalog_rewrite::{
    is_database_catalog_entry, rewrite_database_data_entry, validate_database_catalog_entry,
};
pub(super) use databases::DatabaseFamily;
pub use databases::StagedDatabaseFamily;
pub mod documents;
#[cfg(feature = "storage-test-hooks")]
pub use databases::create_retry_test_hooks;
#[cfg(all(
    feature = "replay-benchmark-observation",
    any(test, feature = "storage-test-hooks")
))]
pub use transactions::{
    inject_native_commit_unknown_after_success, inject_native_commit_unknown_before_commit,
};
mod failure_points;
pub(crate) use failure_points::{
    check_document_write_failure_point, check_document_write_storage_failure_injection,
};
#[doc(hidden)]
pub use failure_points::{
    document_write_failure_point_test_guard, document_write_storage_failures_remaining,
    set_document_write_conflicts_remaining, set_document_write_failure_point,
    set_document_write_storage_failures, DocumentWriteFailurePoint, DocumentWriteStorageFailure,
};
mod fresh_documents;
mod key_encoding;
mod keys;
#[doc(hidden)]
pub use key_encoding::{
    legacy_graph_prefixes_for_diagnostics, legacy_graph_sample_keys_for_diagnostics,
};
mod layout;
mod legacy_graph_state;
mod property_graphs;
use layout::{
    FamilyScope, RawStorageEntry, DEFAULT_FAMILY_NAME, SCHEMA_FAMILY_NAME, TEMP_FAMILY_NAME,
};
pub use layout::{StorageFamily, StorageLayout};
mod index_publication;
mod maintenance;
pub use maintenance::MaintenanceDebt;
mod unique_constraint_cleanup;
mod unique_constraint_publication;
mod vector_publication;
pub use maintenance::{
    set_fulltext_maintenance_failure_point,
    set_materialized_projection_debt_persistence_failure_point,
};
mod metadata;
mod operational;
mod operator_feedback;
mod projections;
mod query_scan_control;
#[cfg(test)]
mod request_test_support;
pub use query_scan_control::{
    current_query_scan_control_scope, should_cancel_controlled_query_scan, QueryScanControlScope,
};
#[doc(hidden)]
pub use query_scan_control::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
    QueryScanControlTestGuard,
};
mod repair;
mod scalar_indexes;
pub use scalar_indexes::{canonical_field_value, index_key_field_value};
mod storage_config;
pub use scalar_indexes::{ScalarIndexBound, ScalarIndexScanHit, ScalarIndexScanRequest};
mod namespaces;
mod scan_types;
mod schema_cleanup;
#[doc(hidden)]
pub use schema_cleanup::PendingSchemaCleanup;
mod schema_ops;
mod schema_write_control;
#[doc(hidden)]
pub use schema_write_control::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, SchemaWriteConflictTestGuard, SchemaWriteConflictWorkerGuard,
    SchemaWritePausePoint,
};
mod row_id;
#[cfg(test)]
#[path = "schema_ops_tests.rs"]
mod schema_ops_tests;
mod sequences;
mod streaming_scans;
pub use streaming_scans::{
    AccountedDocument, AccountedDocumentPage, MidgeRowCursor, RowTraversal,
};
pub mod time_series_indexes;
mod vector_indexes;
pub use vector_indexes::PersistedIvfFlatTrainingSnapshot;
mod verification;
#[cfg(test)]
#[path = "verification_tests.rs"]
mod verification_tests;

pub fn benchmark_row_key_round_trip(
    relation_id: u64,
    id: &str,
) -> Option<(Vec<u8>, String)> {
    let prefix = key_encoding::row_prefix(relation_id);
    let key = key_encoding::row_key(relation_id, id);
    let decoded = key_encoding::utf8_suffix_after_prefix(&key, &prefix)?;
    Some((key, decoded))
}

pub use column_batches::{
    ControlledColumnBatchScanRequest, ControlledColumnBatchSummaryDecision, EncodedSource,
    EncodedSourceDecision, ValidatedEncodedField,
};
pub use documents::{
    DocumentWriteBatchOptions, DocumentWriteBatchReport, DocumentWriteOp, OrderedRowScanRequest,
};
pub use scan_types::OrderedRowBound;
pub use scan_types::{
    ColumnBatchAggregateDecision, ColumnBatchAggregateOutcome, ColumnBatchAggregateSpec,
    ColumnBatchScanDecision, ColumnBatchScanFallbackReason, ColumnBatchScanFilter,
    ColumnBatchScanOp, ColumnBatchScanOutcome, ColumnBatchScanPredicate,
    ColumnBatchSummaryDecision, DocumentRef, MidgeScanTimings, RowDecode, RowFilter,
};
pub use verification::{
    IntegrityCheckReport, RangeHashRecord, RootHashRecord, RowHashRecord, StoredHashState,
};

pub(crate) fn collect_scan(
    scan: cntryl_midge::ScanIterator<'_>,
) -> Result<Vec<RawStorageEntry>, CassieError> {
    scan.try_collect()
        .map(|entries| {
            entries
                .into_iter()
                .map(|(key, value)| (key.to_vec(), value.to_vec()))
                .collect()
        })
        .map_err(CassieError::from)
}

impl Midge {
    fn load_schema_epoch_from_tx(
        tx: &crate::adapter::AdapterTransaction,
    ) -> Result<u64, CassieError> {
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
        tx: &mut crate::adapter::AdapterTransaction,
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
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(next)
    }

    fn load_collections(
        tx: &crate::adapter::AdapterTransaction,
    ) -> Result<Vec<String>, CassieError> {
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

    fn load_namespaces(
        tx: &crate::adapter::AdapterTransaction,
    ) -> Result<Vec<String>, CassieError> {
        let Some(raw) = tx.get(&Self::namespaces_key()).map_err(CassieError::from)? else {
            return Ok(Vec::new());
        };
        let parsed: Vec<String> =
            serde_json::from_slice(&raw).map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    fn save_collections(
        tx: &mut crate::adapter::AdapterTransaction,
        collections: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(collections)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::collections_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn save_databases(
        tx: &mut crate::adapter::AdapterTransaction,
        databases: &[String],
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(databases).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::databases_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn save_namespaces(
        tx: &mut crate::adapter::AdapterTransaction,
        namespaces: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(namespaces)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::namespaces_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn load_row_schema_from_tx(
        tx: &crate::adapter::AdapterTransaction,
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
                field.normalized_name =
                    cassie_sql::column_identifier::ColumnIdentifierPath::stored_field_key(&field.name);
            }
        }
        Ok(Some(row_schema))
    }

    fn save_row_schema_to_tx(
        tx: &mut crate::adapter::AdapterTransaction,
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
        tx: &crate::adapter::AdapterTransaction,
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
        tx: &mut crate::adapter::AdapterTransaction,
        metadata: &ProjectionMeta,
    ) -> Result<(), CassieError> {
        if let Some(definition) = &metadata.materialized {
            cassie_sql::definition_guard::query(
                definition.definition_semantics_version,
                &definition.query,
            )?;
        }
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

    fn runtime_feedback_key(key: &crate::runtime_feedback::RuntimeFeedbackKey) -> Vec<u8> {
        key_encoding::runtime_feedback_key(crate::runtime_feedback::stable_fingerprint(key))
    }

    fn runtime_feedback_prefix() -> Vec<u8> {
        key_encoding::runtime_feedback_prefix()
    }

    fn load_cardinality_stats_from_tx(
        tx: &crate::adapter::AdapterTransaction,
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
        tx: &mut crate::adapter::AdapterTransaction,
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
        tx: &mut crate::adapter::AdapterTransaction,
        collection: &str,
        schema_version: u32,
    ) -> Result<(), CassieError> {
        let mut metadata = Self::load_projection_metadata_from_tx(tx, collection)?
            .unwrap_or_else(|| ProjectionMeta::new(collection, schema_version));
        metadata.schema_version = schema_version;
        Self::save_projection_metadata_to_tx(tx, &metadata)
    }
}

mod statement_read;
use statement_read::DataReadTransaction;
pub use statement_read::{
    NativeRequestControls, NativeStatementReadScope, NativeStatementSnapshot,
    StorageRequestContext,
};
pub use cassie_core::request::RequestMemoryReservation as QueryMemoryReservation;

#[cfg(all(
    feature = "replay-benchmark-observation",
    any(test, feature = "storage-test-hooks")
))]
pub use transactions::data_policy_test_observer;
#[cfg(all(
    feature = "replay-benchmark-observation",
    any(test, feature = "storage-test-hooks")
))]
pub use transactions::data_policy_test_observer::{
    arm_existing_unknown_on_next_data_submission, clear_existing_unknown_on_next_data_submission,
    install_data_submission_observer, DataSubmission,
};
