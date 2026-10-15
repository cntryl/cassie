// Consolidated integration suite: storage_indexes.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/executor.rs"]
mod support_executor;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "support/database_image_generation.rs"]
mod support_database_image_generation;

#[path = "storage_indexes/databases/image_generation.rs"]
mod database_image_generation;

#[path = "storage_indexes/storage/layout_admission.rs"]
mod layout_marker_admission;
#[path = "support/storage_family_snapshot.rs"]
mod support_storage_family_snapshot;

// Formerly tests/index_publication_recovery.rs.
#[path = "storage_indexes/indexes/index_publication_recovery.rs"]
mod index_publication_recovery;

// Formerly tests/integration_sql_scalar_index_lexkey.rs.
#[path = "storage_indexes/indexes/scalar_index_lexkey.rs"]
mod integration_sql_scalar_index_lexkey;
// Formerly tests/integration_sql_scalar_indexes.rs.
#[path = "storage_indexes/indexes/scalar_indexes/mod.rs"]
mod integration_sql_scalar_indexes;

// Index DDL resolves delimited column names with the same exactness as queries.
#[path = "storage_indexes/indexes/index_field_case.rs"]
mod index_field_case;

#[path = "storage_indexes/indexes/composite_scalar_index_correctness.rs"]
mod composite_scalar_index_correctness;

// Formerly tests/key_encoding_segment_compaction.rs.
#[path = "storage_indexes/key_codec/key_encoding_segment_compaction.rs"]
mod key_encoding_segment_compaction;

// Formerly tests/midge_baseline_database_families.rs.
#[path = "storage_indexes/storage/midge_baseline_database_families.rs"]
mod midge_baseline_database_families;

// Formerly tests/midge_error_paths.rs.
#[path = "storage_indexes/storage/midge_error_paths.rs"]
mod midge_error_paths;

// Formerly tests/midge_layout_bootstrap.rs.
#[path = "storage_indexes/storage/midge_layout_bootstrap.rs"]
mod midge_layout_bootstrap;

// Formerly tests/midge_legacy_migration.rs.
#[path = "storage_indexes/storage/midge_legacy_migration.rs"]
mod midge_legacy_migration;

// Formerly tests/midge_metadata_stats.rs.
#[path = "storage_indexes/storage/midge_metadata_stats.rs"]
mod midge_metadata_stats;

// Formerly tests/midge_namespace_hydration.rs.
#[path = "storage_indexes/storage/midge_namespace_hydration.rs"]
mod midge_namespace_hydration;

// Formerly tests/midge_row_blob_layout.rs.
#[path = "storage_indexes/row_codec/midge_row_blob_layout.rs"]
mod midge_row_blob_layout;

// Formerly tests/operational_ownership.rs.
#[path = "storage_indexes/ownership/operational_ownership.rs"]
mod operational_ownership;

// Formerly tests/snapshot_restore.rs.
#[path = "storage_indexes/snapshots/snapshot_restore.rs"]
mod snapshot_restore;

// Formerly tests/storage_integrity.rs.
#[path = "storage_indexes/integrity/storage_integrity.rs"]
mod storage_integrity;

// Formerly tests/scalar_index_redundant_bounds.rs.
#[path = "storage_indexes/indexes/scalar_index_redundant_bounds.rs"]
mod scalar_index_redundant_bounds;

// A user-declared `id` column is an ordinary stored field: only `_id` names
// the internal row identity, so column-index coverage and projection must
// treat a declared `id` like any other column.
#[path = "storage_indexes/indexes/column_index_declared_id.rs"]
mod column_index_declared_id;

#[path = "storage_indexes/indexes/uuid_bytea_scalar_indexes.rs"]
mod uuid_bytea_scalar_indexes;

#[path = "storage_indexes/storage/column_name_reuse_resolution.rs"]
mod column_name_reuse_resolution;

#[path = "storage_indexes/indexes/row_blob_array_bounds.rs"]
mod row_blob_array_bounds;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "storage_indexes/read_paths/scalar_read_equivalence.rs"]
mod scalar_read_equivalence;

#[path = "storage_indexes/types/temporal/instant_equality.rs"]
mod types_temporal_instant_equality;

#[path = "storage_indexes/operators/family_laws.rs"]
mod operators_family_laws;

#[path = "storage_indexes/text_search/configuration_changes.rs"]
mod text_search_configuration_changes;

#[path = "storage_indexes/types/ranges/operators.rs"]
mod types_ranges_operators;
