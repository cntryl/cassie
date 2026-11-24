pub use cassie_core::catalog::storage_metadata::{
    CollectionMeta, CollectionStorageMode, NamespaceMeta, SUPPORTED_COLLECTION_STORAGE_VERSION,
};

pub use cassie_core::catalog::projection::{
    MaterializedProjectionState, ProjectionComparisonReportMeta, ProjectionFreshness,
    ProjectionHashAlgorithmMeta, ProjectionHashCoverageMeta, ProjectionHashMeta,
    ProjectionIntegrityReportMeta, ProjectionKind, ProjectionRebuildState,
    ProjectionRebuildVerificationMeta, ProjectionSwapMeta, ProjectionVerificationState,
    ProjectionVersionMeta, ProjectionVersionState,
};
pub use cassie_sql::catalog_declarations::{
    materialized_output_collection, MaterializedProjectionMeta, MaterializedProjectionSpec,
    ProjectionMeta, SQL_MATERIALIZED_VIEW_OPTION,
};

#[must_use]
pub(crate) fn is_sql_materialized_view(metadata: &MaterializedProjectionMeta) -> bool {
    metadata
        .options
        .get(crate::catalog::SQL_MATERIALIZED_VIEW_OPTION)
        .is_some_and(|value| value == "true")
}
