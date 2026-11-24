pub mod column_storage;
pub mod composite_type;
pub mod controlled_name;
pub mod declarations;
pub mod domain_type;
pub mod enum_type;
pub mod graph_schema;
pub mod identifier;
pub mod object_comment;
pub mod object_identity;
pub mod operational;
pub mod property_graph;
pub mod projection;
pub mod projection_reports;
pub mod retention;
pub mod role;
pub mod scope;
pub mod sequence;
pub mod storage_metadata;

pub use column_storage::{ColumnCompression, ColumnStorage, ColumnStorageSetting};
pub use composite_type::{data_type_depends_on, CompositeTypeMeta, TypeAlterError, COMPOSITE_TYPE_FORMAT_VERSION};
pub use domain_type::{same_stored_relation, DomainTypeMeta, DOMAIN_TYPE_FORMAT_VERSION};
pub use enum_type::{AddValuePlan, EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
pub use object_comment::{ObjectCommentAddress, ObjectCommentMeta};
pub use operational::{OperationalAssignmentMeta, OperationalAssignmentState};
pub use projection::{
    MaterializedProjectionState, ProjectionComparisonReportMeta, ProjectionFreshness,
    ProjectionHashAlgorithmMeta, ProjectionHashCoverageMeta, ProjectionHashMeta,
    ProjectionIntegrityReportMeta, ProjectionKind, ProjectionRebuildState,
    ProjectionRebuildVerificationMeta, ProjectionSwapMeta, ProjectionVerificationState,
    ProjectionVersionMeta, ProjectionVersionState,
};
pub use property_graph::{
    GraphEdgeMeta, GraphEndpointMeta, GraphKeyColumn, GraphLabelMeta, GraphPropertyMeta,
    GraphVertexMeta, PropertyGraphMeta,
};
pub use retention::{
    RetentionEnforcementMode, RetentionPolicyMeta, RetentionPolicyState, RetentionScheduleState,
};
pub use role::{normalize_role_name, RoleGrantTarget, RoleMeta, RoleObjectGrantMeta};
pub use sequence::{SequenceCursor, SequenceError, SequenceMeta, SequenceOwner, SequenceParameters};
pub use storage_metadata::{
    CollectionCardinalityStats, CollectionMeta, CollectionStorageMode, ColumnBatchChunkMeta,
    ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchNumericSum, ColumnBatchRow,
    ColumnBatchSegmentMeta, FieldCardinalityStats, FieldHeavyHitter, FieldHistogramBucket,
    IndexCardinalityStats, IndexKind, NamespaceMeta, SUPPORTED_COLLECTION_STORAGE_VERSION,
};

#[cfg(test)]
mod storage_metadata_tests;
