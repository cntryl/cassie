// Consolidated integration suite: analytics.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/aggregate_acceleration.rs.
#[path = "analytics/aggregates/aggregate_acceleration.rs"]
mod aggregate_acceleration;

// Formerly tests/analytical_projection_recovery.rs.
#[path = "analytics/projections/analytical_projection_recovery.rs"]
mod analytical_projection_recovery;

// Formerly tests/analytical_projections.rs.
#[path = "analytics/projections/analytical_projections.rs"]
mod analytical_projections;

#[path = "analytics/projections/analytical_projection_schema_invalidation.rs"]
mod analytical_projection_schema_invalidation;

// Formerly tests/cardinality_generation.rs.
#[path = "analytics/statistics/cardinality_generation.rs"]
mod cardinality_generation;

// Formerly tests/cardinality_incremental.rs.
#[path = "analytics/statistics/cardinality_incremental.rs"]
mod cardinality_incremental;

// Formerly tests/column_batch_encoded_aggregates.rs.
#[path = "analytics/aggregates/column_batch_encoded_aggregates.rs"]
mod column_batch_encoded_aggregates;

// Formerly tests/column_batch_encoded_scans.rs.
#[path = "analytics/column_store/column_batch_encoded_scans.rs"]
mod column_batch_encoded_scans;

// Formerly tests/column_batch_format_v1.rs.
#[path = "analytics/column_store/column_batch_format_v1.rs"]
mod column_batch_format_v1;

// Formerly tests/column_batch_incremental_maintenance.rs.
#[path = "analytics/column_store/column_batch_incremental_maintenance.rs"]
mod column_batch_incremental_maintenance;

// Formerly tests/column_batch_resilience.rs.
#[path = "analytics/column_store/column_batch_resilience/mod.rs"]
mod column_batch_resilience;

// Formerly tests/column_batches.rs.
#[path = "analytics/column_store/column_batches.rs"]
mod column_batches;

// Formerly tests/derived_state_recovery.rs.
#[path = "analytics/derived_state/derived_state_recovery.rs"]
mod derived_state_recovery;

// Formerly tests/projection_concurrency.rs.
#[path = "analytics/projections/projection_concurrency.rs"]
mod projection_concurrency;

// Formerly tests/projection_consistency.rs.
#[path = "analytics/projections/projection_consistency.rs"]
mod projection_consistency;

// Formerly tests/projection_diffing.rs.
#[path = "analytics/projections/projection_diffing.rs"]
mod projection_diffing;

// Formerly tests/projection_hash_recovery.rs.
#[path = "analytics/projections/projection_hash_recovery.rs"]
mod projection_hash_recovery;

// Formerly tests/projection_lifecycle.rs.
#[path = "analytics/projections/projection_lifecycle/mod.rs"]
mod projection_lifecycle;

// Formerly tests/projection_repair.rs.
#[path = "analytics/projections/projection_repair.rs"]
mod projection_repair;

// Formerly tests/projection_verification.rs.
#[path = "analytics/projections/projection_verification.rs"]
mod projection_verification;

// Formerly tests/read_model_bounded_join_selection.rs.
#[path = "analytics/read_models/bounded_joins.rs"]
mod read_model_bounded_join_selection;

// Formerly tests/read_model_optimization_round.rs.
#[path = "analytics/read_models/read_model_optimization_round/mod.rs"]
mod read_model_optimization_round;

// Formerly tests/maintenance_memory_controls.rs.
#[path = "analytics/resources/maintenance_memory_controls.rs"]
mod maintenance_memory_controls;

#[path = "support/read_equivalence.rs"]
mod support_read_equivalence;

#[path = "analytics/read_models/offset_substitution.rs"]
mod analytical_offset_substitution;
#[path = "analytics/projections/metadata.rs"]
mod materialized_projection_metadata;

#[path = "analytics/derived_state/derived_read_equivalence.rs"]
mod derived_read_equivalence;

#[path = "support/array_order.rs"]
mod support_array_order;

#[path = "analytics/types/arrays/typed_array_extrema.rs"]
mod typed_array_extrema;

#[path = "analytics/projections/nonfinite_projection_publication.rs"]
mod nonfinite_projection_publication;

#[path = "analytics/column_store/dictionaries.rs"]
mod selected_dictionary_validation;
