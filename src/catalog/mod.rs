pub mod builtin_identity;
pub mod cardinality;
pub mod collections;
pub mod column_defaults;
pub mod composite_alter;
pub mod composite_reconcile;
pub mod composite_types;
pub mod composite_unique;
pub mod consistency;
pub mod constraints;
mod controlled_name;
pub mod domain_types;
pub mod enum_alter;
pub mod enum_types;
pub(crate) use controlled_name::name_matches_controlled;
pub(crate) use controlled_name::trim_checked as trim_catalog_name_controlled;
pub mod function_resolution;
pub mod graphs;
pub mod indexes;
pub mod maintenance;
pub mod metadata;
pub mod object_comment;
pub mod object_identity;
pub mod operational;
pub mod programs;
pub mod relation_row_type;
pub mod repair;
pub mod retention;
pub mod roles;
pub mod rollups;
pub mod schemas;
pub mod scope;
pub mod sequence_state;
pub mod sequences;
pub mod virtual_views;

pub use cardinality::{
    index_cardinality_key, payload_contains_index_membership, payload_contains_vector_membership,
    vector_index_cardinality_key, CollectionCardinalityStats, FieldCardinalityStats,
    FieldHeavyHitter, FieldHistogramBucket, IndexCardinalityStats,
};
pub use collections::{
    materialized_output_collection, CollectionMeta, CollectionStorageMode,
    MaterializedProjectionMeta, MaterializedProjectionSpec, MaterializedProjectionState,
    NamespaceMeta, ProjectionComparisonReportMeta, ProjectionFreshness,
    ProjectionHashAlgorithmMeta, ProjectionHashCoverageMeta, ProjectionHashMeta,
    ProjectionIntegrityReportMeta, ProjectionKind, ProjectionMeta, ProjectionRebuildState,
    ProjectionRebuildVerificationMeta, ProjectionSwapMeta, ProjectionVerificationState,
    ProjectionVersionMeta, ProjectionVersionState,
};
pub use column_defaults::{
    coerce_default_literal, evaluate_volatile_default, parse_volatile_default_expression,
    volatile_default_accepts,
};
pub use composite_types::{CompositeTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION};
pub use composite_unique::{
    composite_primary_key_constraints, composite_primary_key_indexes, composite_unique_constraints,
    composite_unique_indexes, conflicting_unique_membership, enforces_single_column_uniqueness,
    is_composite_primary_key_member, is_composite_unique_member, unique_constraint_indexes,
    CompositeUnique,
};
pub use consistency::{
    ProjectionConsistencyReportMeta, ProjectionManifestHashMetadata,
    ProjectionManifestRangeSummary, ProjectionManifestRootSummary,
    ProjectionManifestRowHashSummary, ProjectionVerificationManifest,
};
pub use constraints::{
    generated_constraint_name, merge_constraint_set, ConstraintCheck, ConstraintOperator,
    DefaultSequenceOwnership, FieldConstraint, IdentityGeneration, NotNullOwnership,
    UniqueNullPolicy,
};
pub use domain_types::{same_stored_relation, DomainTypeMeta, DOMAIN_TYPE_FORMAT_VERSION};
pub use enum_types::{EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
pub use graphs::GraphMeta;
pub use indexes::{
    ColumnBatchChunkMeta, ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchNumericSum,
    ColumnBatchRow, ColumnBatchSegmentMeta, IndexKind, IndexMeta,
};
pub use maintenance::MaintenanceDebtMeta;
pub use metadata::Catalog;
pub(crate) use metadata::{ControlledCatalogSnapshot, FunctionInputScope};
pub use object_comment::{ObjectCommentAddress, ObjectCommentMeta};
pub use operational::{OperationalAssignmentMeta, OperationalAssignmentState};
pub(crate) const SQL_MATERIALIZED_VIEW_OPTION: &str = "cassie.sql_materialized_view";
pub use programs::{
    ExtensionMeta, FunctionArgMeta, FunctionMeta, ProcedureMeta, VectorAnnCatalogIdentityIds,
    VectorCatalogIdentitySet, VectorDistanceOperatorIds, ViewMeta, Volatility,
};
pub use repair::ProjectionRepairReportMeta;
pub use retention::{RetentionEnforcementMode, RetentionPolicyMeta, RetentionPolicyState};
pub use roles::{
    normalize_role_name, RoleGrantTarget, RoleMeta, RoleObjectGrantMeta, RoleObjectPrivilege,
};
pub use rollups::{
    output_collection_name, RollupAggregateMeta, RollupDefinition, RollupMeta, RollupRefreshCursor,
    RollupState,
};
pub use schemas::{CollectionSchema, FieldMeta};
pub use scope::{
    canonical_identifier_component, canonical_relation_name, canonical_schema_name,
    derive_scoped_name, is_reserved_namespace, is_system_schema, local_name, name_matches,
    parse_name, qualifier_variants, relation_belongs_to_database, relation_database_name,
    relation_schema_name, schema_belongs_to_database, schema_database_name, split_identifier_path,
    DatabaseMeta, ParsedName, RelationId, SchemaId, DEFAULT_SCHEMA, INFORMATION_SCHEMA,
    PG_CATALOG_SCHEMA,
};
pub use sequences::{
    canonical_nextval_expression, parse_nextval_default_expression, serial_sequence_name,
    SequenceCursor, SequenceError, SequenceMeta, SequenceOptions, SequenceOwner,
    SequenceParameters, SequencePersistence,
};
