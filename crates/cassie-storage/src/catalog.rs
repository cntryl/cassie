//! Catalog declarations and names used by the persisted storage adapter.

pub(crate) use cassie_core::catalog::{
    CollectionCardinalityStats, CollectionMeta, CollectionStorageMode, ColumnBatchChunkMeta,
    ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchNumericSum, ColumnBatchRow,
    ColumnBatchSegmentMeta, CompositeTypeMeta, DomainTypeMeta, EnumTypeMeta,
    FieldCardinalityStats, FieldHeavyHitter, FieldHistogramBucket,
    NamespaceMeta, ObjectCommentAddress, ObjectCommentMeta, OperationalAssignmentMeta,
    ProjectionComparisonReportMeta, ProjectionFreshness, ProjectionHashAlgorithmMeta,
    ProjectionHashCoverageMeta, ProjectionHashMeta, ProjectionKind, ProjectionVerificationState,
    PropertyGraphMeta, RetentionPolicyMeta, RoleGrantTarget, RoleMeta, RoleObjectGrantMeta,
    SequenceMeta, SequenceOwner, COMPOSITE_TYPE_FORMAT_VERSION, DOMAIN_TYPE_FORMAT_VERSION,
    ENUM_TYPE_FORMAT_VERSION,
};
pub(crate) use cassie_core::catalog::declarations::{RoleObjectPrivilege, SequencePersistence};
pub(crate) use cassie_core::catalog::projection_reports::{
    ProjectionConsistencyReportMeta, ProjectionRepairReportMeta,
};
pub(crate) use cassie_core::catalog::scope::{
    canonical_relation_name, canonical_schema_name, is_system_schema, local_name, name_matches,
    parse_name, relation_belongs_to_database, relation_database_name, schema_database_name,
    DatabaseMeta, ParsedName, DEFAULT_SCHEMA,
};
pub(crate) use cassie_core::catalog::{graph_schema, object_identity};
pub(crate) use cassie_sql::catalog_declarations::{IndexKind, IndexMeta, ProjectionMeta};
pub(crate) use cassie_sql::composite_unique::enforces_single_column_uniqueness;
pub(crate) use cassie_sql::rollup_declarations::RollupMeta;
pub(crate) use cassie_sql::routine_declarations::{ExtensionMeta, FunctionMeta, ProcedureMeta, ViewMeta};
#[cfg(test)]
pub(crate) use cassie_sql::sequence_default::canonical_nextval_expression;

pub(crate) use crate::cardinality::{payload_contains_index_membership, payload_contains_vector_membership};

pub(crate) type FieldConstraint<A = ()> = cassie_sql::declarations::FieldConstraint<A>;

pub(crate) mod composite_alter {
    pub(crate) use cassie_sql::type_publication::CompositeGroup;
}

pub(crate) mod enum_reconcile {
    pub(crate) use cassie_sql::type_publication::EnumPropagation;
}

#[cfg(test)]
pub(crate) use cassie_core::catalog::declarations::IdentityGeneration;
#[cfg(test)]
pub(crate) use cassie_sql::declarations::DefaultSequenceOwnership;
#[cfg(test)]
pub(crate) mod collections {
    pub(crate) use cassie_core::catalog::{CollectionMeta, CollectionStorageMode};
}
