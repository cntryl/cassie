use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use cntryl_midge::{ColumnFamilyHandle, Engine, Query, TransactionMode, WriteOptions};
use uuid::Uuid;

use crate::app::CassieError;
use crate::catalog::{
    payload_contains_index_membership, payload_contains_vector_membership,
    CollectionCardinalityStats, CollectionMeta, CollectionStorageMode, ColumnBatchCodecMeta,
    ColumnBatchColumn, ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchPayload,
    ColumnBatchRow, ColumnBatchSegmentMeta, ColumnBatchValueRun, FieldCardinalityStats,
    FieldConstraint, FieldHeavyHitter, FieldHistogramBucket, IndexKind, IndexMeta, NamespaceMeta,
    OperationalAssignmentMeta, ProjectionMeta, RetentionPolicyMeta, RoleMeta, RollupMeta,
};
use crate::embeddings::{NormalizedVectorRecord, VectorIndexRecord};
use crate::midge::row_blob::{
    decode_projected_row, decode_projected_row_matching, decode_row, encode_row, RowSchema,
};
use crate::types::{DataType, FieldSchema, Schema, Value, Vector};
use crate::vector::normalize as normalize_vector;

pub struct Midge {
    engine: Engine,
    storage_layout: OnceLock<StorageLayout>,
}

#[derive(Debug, Clone)]
pub struct DocumentRef {
    pub id: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowDecode {
    Full,
    Projected(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowFilter {
    pub field: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnBatchScanFilter {
    pub predicates: Vec<ColumnBatchScanPredicate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnBatchScanPredicate {
    pub field: String,
    pub op: ColumnBatchScanOp,
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBatchScanOp {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, Copy)]
pub enum ColumnBatchScanFallbackReason {
    NoCoveringIndex,
    MissingMetadata,
    SegmentSizeMismatch,
    FieldCoverageMismatch,
    SegmentMissing,
    SegmentChecksumMismatch,
    InvalidPayload,
    InvalidEncodingVersion,
    SegmentCodecMismatch,
    SegmentDecodeFailed,
    RowFilterMismatch,
}

impl ColumnBatchScanFallbackReason {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NoCoveringIndex => "no_covering_column_index",
            Self::MissingMetadata => "missing_metadata",
            Self::SegmentSizeMismatch => "segment_size_mismatch",
            Self::FieldCoverageMismatch => "field_coverage_mismatch",
            Self::SegmentMissing => "segment_missing",
            Self::SegmentChecksumMismatch => "segment_checksum_mismatch",
            Self::InvalidPayload => "invalid_payload",
            Self::InvalidEncodingVersion => "invalid_encoding_version",
            Self::SegmentCodecMismatch => "segment_codec_mismatch",
            Self::SegmentDecodeFailed => "segment_decode_failed",
            Self::RowFilterMismatch => "row_filter_mismatch",
        }
    }

    pub const fn is_decode_fallback(&self) -> bool {
        matches!(
            self,
            Self::SegmentMissing
                | Self::SegmentChecksumMismatch
                | Self::InvalidPayload
                | Self::InvalidEncodingVersion
                | Self::SegmentCodecMismatch
                | Self::SegmentDecodeFailed
        )
    }
}

#[derive(Debug, Clone)]
pub enum ColumnBatchScanDecision {
    Hit(ColumnBatchScanOutcome),
    Fallback(ColumnBatchScanFallbackReason),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MidgeScanTimings {
    pub scan: Duration,
    pub row_decode: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct OrderedRowBound {
    pub id: String,
    pub inclusive: bool,
}

#[derive(Debug, Clone)]
pub struct ColumnBatchScanOutcome {
    pub batches: Vec<Vec<DocumentRef>>,
    pub timings: MidgeScanTimings,
    pub index_name: String,
    pub compressed_bytes: usize,
    pub uncompressed_bytes: usize,
    pub skipped_segments: usize,
    pub decoded_columns: usize,
}

#[path = "adapter/capacity.rs"]
mod capacity;
#[path = "adapter/cardinality_stats.rs"]
mod cardinality_stats;
#[path = "adapter/column_batches.rs"]
mod column_batches;
#[path = "adapter/column_store.rs"]
mod column_store;
#[path = "adapter/documents.rs"]
pub(crate) mod documents;
#[path = "adapter/graphs.rs"]
mod graphs;
#[path = "adapter/key_encoding.rs"]
mod key_encoding;
#[path = "adapter/layout.rs"]
mod layout;
use layout::{
    allow_memory_fallback, FamilyScope, RawStorageEntry, DATA_FAMILY_NAME, DEFAULT_FAMILY_NAME,
    SCHEMA_FAMILY_NAME, TEMP_FAMILY_NAME,
};
pub use layout::{StorageFamily, StorageLayout};
#[path = "adapter/metadata.rs"]
mod metadata;
#[path = "adapter/operational.rs"]
mod operational;
#[path = "adapter/operator_feedback.rs"]
mod operator_feedback;
#[path = "adapter/projections.rs"]
mod projections;
#[path = "adapter/repair.rs"]
mod repair;
#[path = "adapter/scalar_indexes.rs"]
mod scalar_indexes;
pub(crate) use scalar_indexes::{ScalarIndexBound, ScalarIndexScanRequest};
#[path = "adapter/schema_ops.rs"]
mod schema_ops;
#[path = "adapter/sequences.rs"]
mod sequences;
#[path = "adapter/time_series_indexes.rs"]
pub(crate) mod time_series_indexes;
#[path = "adapter/verification.rs"]
mod verification;

pub(crate) use documents::{DocumentWriteBatchOptions, DocumentWriteOp};
pub(crate) use graphs::GraphEdgeRecord;
pub use verification::{
    IntegrityCheckReport, RangeHashRecord, RootHashRecord, RowHashRecord, StoredHashState,
};

impl Midge {
    pub fn new() -> Result<Self, CassieError> {
        let data_dir =
            env::var("CASSIE_MIDGE_DATA_DIR").unwrap_or_else(|_| "./.cassie/midge".to_string());
        Self::new_with_data_dir(data_dir)
    }

    pub fn new_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref()).build();

        let engine = match Engine::open(options) {
            Ok(engine) => engine,
            Err(error) => {
                if allow_memory_fallback() {
                    Engine::open(cntryl_midge::OpenOptions::in_memory().build())
                        .map_err(CassieError::from)?
                } else {
                    return Err(CassieError::from(error));
                }
            }
        };

        Ok(Self {
            engine,
            storage_layout: OnceLock::new(),
        })
    }

    pub fn new_strict_with_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, CassieError> {
        let options = cntryl_midge::OpenOptions::local(data_dir.as_ref()).build();
        Ok(Self {
            engine: Engine::open(options).map_err(CassieError::from)?,
            storage_layout: OnceLock::new(),
        })
    }

    pub fn bootstrap_families(&self) -> Result<StorageLayout, CassieError> {
        let schema = self.get_or_create_family(StorageFamily::Schema)?;
        let data = self.get_or_create_family(StorageFamily::Data)?;
        let temp = self.get_or_create_family(StorageFamily::Temp)?;

        if schema.id() == data.id() || schema.id() == temp.id() || data.id() == temp.id() {
            return Err(CassieError::StorageBootstrap(
                "family ids must be distinct for schema/data/temp families".to_string(),
            ));
        }

        Ok(StorageLayout { schema, data, temp })
    }

    pub fn ensure_families_ready(&self) -> Result<&StorageLayout, CassieError> {
        if self.storage_layout.get().is_none() {
            let layout = self.bootstrap_families()?;
            self.ensure_lexkey_layout_ready(&layout)?;
            let _ = self.storage_layout.set(layout);
        }

        self.storage_layout.get().ok_or_else(|| {
            CassieError::StorageBootstrap("failed to initialize midge storage families".to_string())
        })
    }

    fn ensure_lexkey_layout_ready(&self, layout: &StorageLayout) -> Result<(), CassieError> {
        self.reject_legacy_layout_prefixes(layout)?;

        let marker_key = key_encoding::layout_marker_key();
        let mut tx = self
            .engine
            .begin_tx(layout.schema.id(), TransactionMode::ReadWrite)
            .map_err(CassieError::from)?;
        match tx.get(&marker_key).map_err(CassieError::from)? {
            Some(value) if value == key_encoding::LAYOUT_MARKER_VALUE => Ok(()),
            Some(value) => Err(CassieError::StorageBootstrap(format!(
                "incompatible lexkey v{} storage layout marker: {:?}",
                key_encoding::LAYOUT_VERSION,
                String::from_utf8_lossy(&value)
            ))),
            None => {
                tx.put(marker_key, key_encoding::LAYOUT_MARKER_VALUE.to_vec(), None)
                    .map_err(CassieError::from)?;
                tx.commit(WriteOptions::sync()).map_err(CassieError::from)
            }
        }
    }

    fn reject_legacy_layout_prefixes(&self, layout: &StorageLayout) -> Result<(), CassieError> {
        for (family_name, family_id, prefixes) in [
            (
                SCHEMA_FAMILY_NAME,
                layout.schema.id(),
                key_encoding::LEGACY_SCHEMA_PREFIXES,
            ),
            (
                DATA_FAMILY_NAME,
                layout.data.id(),
                key_encoding::LEGACY_DATA_PREFIXES,
            ),
            (
                TEMP_FAMILY_NAME,
                layout.temp.id(),
                key_encoding::LEGACY_TEMP_PREFIXES,
            ),
        ] {
            let tx = self
                .engine
                .begin_tx(family_id, TransactionMode::ReadOnly)
                .map_err(CassieError::from)?;
            for prefix in prefixes {
                let mut scan = tx
                    .scan(&Query::new().prefix(prefix.to_vec().into()))
                    .map_err(CassieError::from)?;
                if scan.next().is_some() {
                    return Err(CassieError::StorageBootstrap(format!(
                        "incompatible lexkey v{} storage layout: found v1 key prefix '{}' in {family_name}; recreate the Midge data directory",
                        key_encoding::LAYOUT_VERSION,
                        String::from_utf8_lossy(prefix)
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn storage_layout(&self) -> Option<StorageLayout> {
        self.storage_layout.get().cloned()
    }

    pub fn schema_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Schema], mode)
    }

    pub fn data_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Data], mode)
    }

    pub fn temp_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Temp], mode)
    }

    pub fn default_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction_by_name(DEFAULT_FAMILY_NAME, mode)
    }

    pub fn begin_families_tx(
        &self,
        families: &[StorageFamily],
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let scope = FamilyScope::for_families(families)?;
        let family = scope.family().ok_or_else(|| {
            CassieError::Unsupported(
                "transactions currently support exactly one storage family".to_string(),
            )
        })?;

        self.transaction(family, mode)
    }

    fn transaction(
        &self,
        family: StorageFamily,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let layout = self.ensure_families_ready()?;
        let cf = match family {
            StorageFamily::Schema => &layout.schema,
            StorageFamily::Data => &layout.data,
            StorageFamily::Temp => &layout.temp,
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    fn transaction_by_name(
        &self,
        family: &str,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let Some(cf) = self.engine.get_column_family(family) else {
            return Err(CassieError::StorageMissingFamily(format!(
                "required column family '{family}' is missing"
            )));
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    fn get_or_create_family(
        &self,
        family: StorageFamily,
    ) -> Result<ColumnFamilyHandle, CassieError> {
        let name = family.name();
        if let Some(existing) = self.engine.get_column_family(name) {
            return Ok(existing);
        }

        if let Ok(created) = self.engine.create_column_family(name) {
            return Ok(created);
        }

        self.engine.get_column_family(name).ok_or_else(|| {
            CassieError::StorageBootstrap(format!("cannot resolve required column family '{name}'"))
        })
    }

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

    fn begin_schema_readonly_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadOnly)
    }

    fn begin_schema_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadWrite)
    }

    fn begin_data_readonly_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.data_tx(TransactionMode::ReadOnly)
    }

    fn begin_data_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.data_tx(TransactionMode::ReadWrite)
    }

    pub fn raw_get(
        &self,
        family: StorageFamily,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, CassieError> {
        let tx = self.transaction(family, TransactionMode::ReadOnly)?;
        let value = tx.get(key).map_err(CassieError::from)?;
        Ok(value.map(|value| value.to_vec()))
    }

    pub fn raw_scan_prefix(
        &self,
        family: StorageFamily,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let tx = self.transaction(family, TransactionMode::ReadOnly)?;
        let mut iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        let mut values = Vec::new();
        while let Some((key, value)) = iterator.next() {
            values.push((key, value));
        }
        Ok(values)
    }

    pub fn raw_scan_prefix_named(
        &self,
        family: &str,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let tx = self.transaction_by_name(family, TransactionMode::ReadOnly)?;
        let mut iterator = tx
            .scan(&Query::new().prefix(prefix.to_vec().into()))
            .map_err(CassieError::from)?;

        let mut values = Vec::new();
        while let Some((key, value)) = iterator.next() {
            values.push((key, value));
        }
        Ok(values)
    }

    pub fn clear_temp_family(&self) -> Result<usize, CassieError> {
        let mut tx = self.temp_tx(TransactionMode::ReadWrite)?;
        let mut iterator = tx.scan(&Query::new()).map_err(CassieError::from)?;
        let mut keys = Vec::new();
        while let Some((raw_key, _)) = iterator.next() {
            keys.push(raw_key);
        }

        if keys.is_empty() {
            return Ok(0);
        }

        let deleted = keys.len();
        for key in keys {
            tx.delete(key).map_err(CassieError::from)?;
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(deleted)
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

    pub fn schema_epoch(&self) -> Result<u64, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_schema_epoch_from_tx(&tx)
    }

    pub fn bump_schema_epoch(&self) -> Result<u64, CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let next = Self::load_schema_epoch_from_tx(&tx)?.saturating_add(1);
        Self::save_schema_epoch_to_tx(&mut tx, next)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(next)
    }

    fn load_collections(&self, tx: &cntryl_midge::Transaction) -> Result<Vec<String>, CassieError> {
        let raw = tx
            .get(&Self::collections_key())
            .map_err(CassieError::from)?;
        if raw.is_none() {
            return Ok(Vec::new());
        }
        let parsed: Vec<String> = serde_json::from_slice(&raw.unwrap())
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    fn load_namespaces(&self, tx: &cntryl_midge::Transaction) -> Result<Vec<String>, CassieError> {
        let raw = tx.get(&Self::namespaces_key()).map_err(CassieError::from)?;
        if raw.is_none() {
            return Ok(Vec::new());
        }
        let parsed: Vec<String> = serde_json::from_slice(&raw.unwrap())
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(parsed)
    }

    fn save_collections(
        &self,
        tx: &mut cntryl_midge::Transaction,
        collections: &[String],
    ) -> Result<(), CassieError> {
        let value = serde_json::to_vec(collections)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::collections_key(), value, None)
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn save_namespaces(
        &self,
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
        let tx = self.begin_schema_readonly_tx()?;
        if let Some(row_schema) = Self::load_row_schema_from_tx(&tx, collection)? {
            return Ok(row_schema);
        }

        let raw = tx
            .get(&Self::collection_schema_key(collection))
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
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

pub fn vector_from_json(value: &serde_json::Value) -> Option<Vector> {
    let arr = value.as_array()?;
    let mut nums = Vec::with_capacity(arr.len());
    for n in arr {
        nums.push(n.as_f64()? as f32);
    }
    Some(Vector::new(nums))
}
