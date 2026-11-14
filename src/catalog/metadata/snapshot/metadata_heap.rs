//! Exhaustive owned-field accounting for retained catalog metadata.

use super::heap::HeapBytes;

impl HeapBytes for crate::catalog::ObjectCommentMeta {
    fn heap_bytes(
        &self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<usize, crate::app::CassieError> {
        controls.check_interruption()?;
        crate::executor::retained_memory::add(
            self.address.database.heap_bytes(controls)?,
            self.description.heap_bytes(controls)?,
        )
    }
}

impl HeapBytes for crate::catalog::RoleGrantTarget {
    fn heap_bytes(
        &self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<usize, crate::app::CassieError> {
        controls.check_interruption()?;
        Ok(0)
    }
}

impl crate::catalog::metadata::snapshot::exact_eq::ExactMetadataEq
    for crate::catalog::RoleGrantTarget
{
    fn exact_eq(
        &self,
        other: &Self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<bool, crate::app::CassieError> {
        controls.check_interruption()?;
        Ok(self == other)
    }
}

unit_enum_heap!(crate::catalog::RoleObjectPrivilege {
    Usage,
    Select,
    Insert,
    Update,
    Delete
});

metadata_heap!(crate::catalog::RoleObjectGrantMeta {
    database_family,
    role_id,
    target,
    privileges,
});

metadata_heap!(crate::catalog::cardinality::CollectionCardinalityStats {
    built_generation,
    row_count,
    hydrated,
    indexes,
    fields,
});

metadata_heap!(crate::catalog::collections::CollectionMeta {
    name,
    storage_id,
    row_type_object_id,
    row_array_object_id,
    row_type_revision,
    has_ever_had_indexes,
    description,
    storage_mode,
    storage_version,
});

metadata_heap!(crate::catalog::schemas::CollectionSchema { collection, fields });

unit_enum_heap!(crate::catalog::collections::CollectionStorageMode {
    RowStore,
    ColumnIndexed,
    ColumnStore
});

metadata_heap!(crate::catalog::constraints::ConstraintCheck {
    field,
    operator,
    value,
    bound_expression,
});

unit_enum_heap!(crate::catalog::constraints::ConstraintOperator {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Like,
    IsDistinctFrom,
    IsNotDistinctFrom
});

metadata_heap!(crate::catalog::scope::DatabaseMeta {
    name,
    description,
    physical_family,
});

metadata_heap!(crate::catalog::cardinality::FieldCardinalityStats {
    non_null_count,
    null_count,
    missing_count,
    distinct_count,
    min_value,
    max_value,
    sample_count,
    histogram_buckets,
    heavy_hitters,
    confidence,
    stale_reason,
});

metadata_heap!(crate::catalog::constraints::FieldConstraint {
    definition_semantics_version,
    field,
    primary_key_name,
    primary_key_ordinal,
    unique_name,
    unique_ordinal,
    constraint_index_name,
    constraint_index_include_fields,
    constraint_index_options,
    check_name,
    foreign_key_name,
    foreign_key_ordinal,
    foreign_key_on_delete,
    foreign_key_on_update,
    not_null,
    not_null_ownership,
    unique,
    unique_null_policy,
    primary_key,
    default_value,
    default_expression,
    default_sequence,
    default_sequence_owned,
    check,
    references_table,
    references_field,
    identity,
    identity_options,
    storage,
    compression,
});

metadata_heap!(crate::catalog::cardinality::FieldHeavyHitter { value, count });

metadata_heap!(crate::catalog::cardinality::FieldHistogramBucket {
    lower,
    upper,
    count,
});

metadata_heap!(crate::catalog::schemas::FieldMeta {
    name,
    data_type,
    domain,
    is_indexed,
    boost,
});

metadata_heap!(crate::catalog::programs::FunctionArgMeta { name, data_type });

metadata_heap!(crate::catalog::programs::FunctionMeta {
    name,
    args,
    return_type,
    volatility,
    body,
    object_id,
});

metadata_heap!(crate::catalog::programs::ExtensionMeta {
    format_version,
    database,
    name,
    version,
    schema,
    owner_oid,
    object_id,
    digest_text_object_id,
    digest_bytea_object_id,
    vector_type_object_id,
    vector_catalog_identities,
});

metadata_heap!(crate::types::composite::CompositeField { name, data_type });

metadata_heap!(crate::catalog::composite_types::CompositeTypeMeta {
    format_version,
    database,
    name,
    object_id,
    array_object_id,
    relation_object_id,
    owner_oid,
    revision,
    fields,
    dropped_positions,
});

metadata_heap!(crate::catalog::programs::VectorCatalogIdentitySet {
    operators,
    hnsw,
    ivfflat,
});

metadata_heap!(crate::catalog::programs::VectorDistanceOperatorIds {
    l2,
    inner_product,
    cosine,
});

metadata_heap!(crate::catalog::programs::VectorAnnCatalogIdentityIds {
    access_method,
    l2_operator_family,
    inner_product_operator_family,
    cosine_operator_family,
    l2_operator_class,
    inner_product_operator_class,
    cosine_operator_class,
});

metadata_heap!(crate::catalog::graphs::GraphMeta {
    name,
    storage_id,
    node_collection,
    edge_collection,
    node_type_field,
    node_id_field,
    edge_id_field,
    source_type_field,
    source_id_field,
    target_type_field,
    target_id_field,
    edge_type_field,
    weight_field,
});

unit_enum_heap!(crate::catalog::constraints::IdentityGeneration { Always, ByDefault });

metadata_heap!(crate::catalog::cardinality::IndexCardinalityStats { cardinality });

unit_enum_heap!(crate::catalog::indexes::IndexKind {
    Scalar,
    FullText,
    Vector,
    Hybrid,
    Column,
    TimeSeries
});

metadata_heap!(crate::catalog::indexes::IndexMeta {
    definition_semantics_version,
    collection,
    name,
    field,
    fields,
    expressions,
    include_fields,
    predicate,
    kind,
    unique,
    options,
});

metadata_heap!(crate::catalog::maintenance::MaintenanceDebtMeta {
    collection,
    artifact,
    target_generation,
    retry_count,
    last_error,
    fallback_reason,
});

metadata_heap!(crate::catalog::collections::MaterializedProjectionMeta {
    definition_semantics_version,
    name,
    query,
    options,
    output_collection,
    source_collections,
    schema_epoch,
    output_schema,
    state,
    definition_fingerprint,
    refresh_cursor,
    last_built_ms,
});

unit_enum_heap!(crate::catalog::collections::MaterializedProjectionState {
    Building,
    Ready,
    Stale,
    Failed
});

metadata_heap!(crate::catalog::collections::NamespaceMeta {
    name,
    description,
    object_id,
});

unit_enum_heap!(crate::catalog::constraints::NotNullOwnership {
    None,
    Explicit,
    PrimaryKey,
    ExplicitAndPrimaryKey
});

metadata_heap!(crate::catalog::operational::OperationalAssignmentMeta {
    assignment_id,
    node_id,
    projection_id,
    tenant,
    partition_key,
    generation,
    state,
    routing_hint,
    updated_ms,
});

unit_enum_heap!(crate::catalog::operational::OperationalAssignmentState {
    Claimed,
    Draining,
    Released,
    Failed
});

metadata_heap!(crate::catalog::programs::ProcedureMeta {
    name,
    args,
    body,
    object_id,
});

metadata_heap!(
    crate::catalog::collections::ProjectionComparisonReportMeta {
        report_id,
        created_ms,
        target,
        target_version_id,
        state,
        compatibility_status,
        root_digest,
        manifest_digest,
        mismatch_count,
        unverifiable_count,
        diagnostic_sample,
        last_error,
    }
);

metadata_heap!(
    crate::catalog::consistency::ProjectionConsistencyReportMeta {
        report_id,
        created_ms,
        projection_id,
        projection_version_id,
        state,
        compatibility_status,
        manifest_count,
        instance_ids,
        root_digest,
        manifest_digest,
        mismatch_count,
        divergent_range_count,
        divergent_row_count,
        stale_manifest_count,
        incompatible_manifest_count,
        unverifiable_count,
        diagnostic_sample,
        last_error,
    }
);

unit_enum_heap!(crate::catalog::collections::ProjectionFreshness {
    Unknown,
    Fresh,
    Stale,
    Rebuilding,
    Failed
});

metadata_heap!(crate::catalog::collections::ProjectionHashAlgorithmMeta {
    algorithm,
    digest_length,
    canonical_encoder_version,
    hash_version,
});

metadata_heap!(crate::catalog::collections::ProjectionHashCoverageMeta {
    state,
    row_count,
    range_count,
    source_checkpoint,
    projection_version_id,
    last_computed_ms,
    digest,
    last_error,
});

metadata_heap!(crate::catalog::collections::ProjectionHashMeta {
    algorithm,
    rows,
    ranges,
    root,
});

metadata_heap!(crate::catalog::collections::ProjectionIntegrityReportMeta {
    state,
    target,
    version_id,
    mode,
    checked_components,
    skipped_components,
    mismatch_count,
    missing_count,
    stale_count,
    repairable,
    elapsed_ms,
    completed_ms,
    last_error,
});

unit_enum_heap!(crate::catalog::collections::ProjectionKind {
    Collection,
    Materialized
});

metadata_heap!(crate::catalog::collections::ProjectionMeta {
    projection_id,
    collection,
    kind,
    schema_version,
    offset,
    lag,
    rebuild_state,
    source_identity,
    source_generations,
    source_checkpoint,
    source_position,
    last_applied_event_id,
    replay_batch_id,
    applied_event_count,
    skipped_duplicate_count,
    freshness,
    last_error,
    materialized,
    versions,
    active_version,
    swap,
    hashes,
    verification,
    integrity,
    version_high_water,
});

unit_enum_heap!(crate::catalog::collections::ProjectionRebuildState {
    Idle,
    Rebuilding,
    Failed
});

metadata_heap!(
    crate::catalog::collections::ProjectionRebuildVerificationMeta {
        state,
        started_ms,
        completed_ms,
        mismatch_count,
        unverifiable_ranges,
        failure_reason,
    }
);

metadata_heap!(crate::catalog::repair::ProjectionRepairReportMeta {
    report_id,
    created_ms,
    projection_name,
    target,
    version_id,
    scope,
    action,
    state,
    executable,
    affected_objects,
    source_report_state,
    source_mismatch_count,
    source_missing_count,
    source_stale_count,
    verification_required,
    post_verification_state,
    last_error,
});

metadata_heap!(crate::catalog::collections::ProjectionSwapMeta {
    target_version_id,
    previous_version_id,
    swapped_at_ms,
    unsafe_override,
    last_error,
});

unit_enum_heap!(crate::catalog::collections::ProjectionVerificationState {
    Unknown,
    Current,
    Stale,
    Missing,
    Incomplete,
    Incompatible,
    Empty,
    Pending,
    Running,
    Verified,
    Failed,
    Unverifiable,
    Skipped
});

metadata_heap!(crate::catalog::collections::ProjectionVersionMeta {
    version_id,
    output_collection,
    definition_fingerprint,
    source_schema_epoch,
    state,
    created_ms,
    activated_ms,
    retired_ms,
    last_error,
    verification,
});

unit_enum_heap!(crate::catalog::collections::ProjectionVersionState {
    Building,
    Built,
    Active,
    Failed,
    Retired
});

unit_enum_heap!(crate::catalog::retention::RetentionEnforcementMode { Explicit });

metadata_heap!(crate::catalog::retention::RetentionPolicyMeta {
    name,
    collection,
    timestamp_field,
    retention_duration,
    enforcement_mode,
    state,
    last_enforced_ms,
    last_deleted_rows,
    last_skipped_rows,
    last_error,
});

unit_enum_heap!(crate::catalog::retention::RetentionPolicyState { Ready, Error });

metadata_heap!(crate::catalog::roles::RoleMeta {
    name,
    can_login,
    is_admin,
    password_hash,
    database_grants,
    object_id,
});

metadata_heap!(crate::catalog::rollups::RollupAggregateMeta {
    alias,
    function,
    expression,
    data_type,
});

metadata_heap!(crate::catalog::rollups::RollupMeta {
    definition_semantics_version,
    name,
    source_collection,
    output_collection,
    timestamp_field,
    bucket_width,
    origin,
    bucket_expr,
    group_keys,
    aggregates,
    filter_expr,
    version,
    state,
    refresh_cursor,
});

metadata_heap!(crate::catalog::rollups::RollupRefreshCursor {
    source_generation,
    source_epoch,
    last_refresh_ms,
    source_row_count,
    lag_rows,
});

unit_enum_heap!(crate::catalog::rollups::RollupState {
    Building,
    Ready,
    Stale
});

metadata_heap!(crate::catalog::sequences::SequenceMeta {
    name,
    data_type,
    start_value,
    increment_by,
    current_value,
    is_called,
    min_value,
    max_value,
    cache,
    cycle,
    owned_by,
    persistence,
    object_id,
});

metadata_heap!(crate::catalog::sequence_state::SequenceOptions {
    data_type,
    increment,
    min_value,
    max_value,
    start,
    restart,
    cache,
    cycle,
});

metadata_heap!(crate::catalog::sequences::SequenceOwner { table, column });

unit_enum_heap!(crate::catalog::sequences::SequencePersistence {
    Permanent,
    Unlogged,
    Temporary
});

unit_enum_heap!(crate::catalog::constraints::UniqueNullPolicy {
    Distinct,
    NotDistinct
});

metadata_heap!(crate::catalog::programs::ViewMeta {
    definition_semantics_version,
    name,
    query,
    schema,
    object_id,
    extension_dependency_ids,
});

unit_enum_heap!(crate::catalog::programs::Volatility {
    Immutable,
    Stable,
    Volatile
});

// A domain descriptor is shared behind an `Arc`; a clone retains no copy.
impl HeapBytes for crate::types::DomainRef {
    fn heap_bytes(
        &self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<usize, crate::app::CassieError> {
        controls.check_interruption()?;
        Ok(0)
    }
}

impl crate::catalog::metadata::snapshot::exact_eq::ExactMetadataEq for crate::types::DomainRef {
    fn exact_eq(
        &self,
        other: &Self,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<bool, crate::app::CassieError> {
        controls.check_interruption()?;
        Ok(self == other)
    }
}

metadata_heap!(crate::types::DomainDefault {
    sql,
    value,
    expression,
});

metadata_heap!(crate::types::DomainCheck {
    name,
    sql,
    validated,
});

metadata_heap!(crate::catalog::domain_types::DomainTypeMeta {
    format_version,
    database,
    name,
    object_id,
    array_object_id,
    owner_oid,
    revision,
    base,
    not_null,
    default,
    checks,
});

metadata_heap!(crate::types::EnumMember {
    member_id,
    label,
    sort_order,
    uncommitted,
});

metadata_heap!(crate::catalog::enum_types::EnumTypeMeta {
    format_version,
    database,
    name,
    object_id,
    array_object_id,
    owner_oid,
    revision,
    members,
});

unit_enum_heap!(crate::catalog::column_storage::ColumnStorage {
    Plain,
    External,
    Extended,
    Main,
});

unit_enum_heap!(crate::catalog::column_storage::ColumnCompression { Pglz, Lz4 });
