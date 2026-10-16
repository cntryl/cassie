#[path = "pgwire_extended/types/numeric/mixed_numeric_comparisons.rs"]
mod mixed_numeric_comparisons;
#[path = "support/mixed_numeric.rs"]
mod support_mixed_numeric;

#[path = "pgwire_extended/mod.rs"]
mod pgwire_extended;
#[path = "pgwire_extended/parameters/occurrences.rs"]
mod pgwire_parameter_occurrences;
#[path = "pgwire_extended/parameters/parameter_slot_completeness.rs"]
mod pgwire_parameter_slot_completeness;
#[path = "pgwire_extended/parameters/unsigned_parameter_oids.rs"]
mod pgwire_unsigned_parameter_oids;
#[path = "pgwire_extended/grammar/rows/arity_aliases.rs"]
mod row_arity_aliases;
#[path = "support/parameter_occurrence_wire.rs"]
mod support_parameter_occurrence_wire;

#[path = "pgwire_extended/parameters/parameter_signatures.rs"]
mod pgwire_parameter_signatures;
#[path = "support/parameter_signature_wire.rs"]
mod support_parameter_signature_wire;

#[path = "pgwire_extended/parameters/parameter_admission.rs"]
mod parameter_admission;

#[path = "pgwire_extended/security/portal_authorization.rs"]
mod portal_authorization;
#[path = "support/portal_authorization.rs"]
mod support_portal_authorization;

#[path = "pgwire_extended/protocol/statement_visibility.rs"]
mod statement_visibility;

#[path = "pgwire_extended/expressions/conditional/conditional_expressions.rs"]
mod conditional_expressions;
#[path = "pgwire_extended/protocol/constant_planning.rs"]
mod constant_planning;

#[path = "pgwire_extended/expressions/conditional/coalesce_promotion.rs"]
mod coalesce_promotion;

#[path = "support/pgwire_transaction_boundary.rs"]
mod support_pgwire_transaction_boundary;
#[path = "pgwire_extended/transactions/transaction_errors.rs"]
mod transaction_errors;

#[path = "pgwire_extended/protocol/vector_carriers.rs"]
mod pgwire_type_vector_carriers;
// Consolidated integration suite: pgwire_extended.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "pgwire_extended/protocol/aliases.rs"]
mod pgwire_aliases;
#[path = "pgwire_extended/protocol/pagination.rs"]
mod pgwire_pagination;
#[path = "pgwire_extended/portals/typed_portal.rs"]
mod pgwire_typed_portal;
#[path = "pgwire_extended/portals/portal_memory_calibration.rs"]
mod portal_memory_calibration;
#[path = "pgwire_extended/conflicts/skipped_conflict_returning.rs"]
mod skipped_conflict_returning;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/portal_memory_calibration.rs"]
mod support_portal_memory_calibration;
#[path = "support/skipped_conflict_returning_wire.rs"]
mod support_skipped_conflict_returning_wire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;
#[path = "support/typed_portal.rs"]
mod support_typed_portal;

#[path = "pgwire_extended/types/json/json_array_nulls.rs"]
mod pgwire_type_json_array_nulls;
#[path = "pgwire_extended/types/json/constructors/errors.rs"]
mod pgwire_type_json_scalar_documents;
#[path = "pgwire_extended/type_resolution/unknown_metadata.rs"]
mod pgwire_type_unknown_metadata;
#[path = "pgwire_extended/types/arrays/vector_array_metadata.rs"]
mod pgwire_type_vector_array_metadata;
#[path = "support/type_metadata_contract.rs"]
mod support_type_metadata_contract;

#[path = "pgwire_extended/types/scalars/scalar_array_matrix.rs"]
mod pgwire_type_scalar_array_matrix;
#[path = "pgwire_extended/types/scalars/scalar_matrix.rs"]
mod pgwire_type_scalar_matrix;
#[path = "support/type_codec_matrix.rs"]
mod support_type_codec_matrix;

#[path = "pgwire_extended/types/arrays/aggregate_identity/semantics.rs"]
mod pgwire_type_array_text_inputs;
#[path = "pgwire_extended/type_resolution/codec_boundaries.rs"]
mod pgwire_type_codec_boundaries;
#[path = "pgwire_extended/type_resolution/declared_null_matrix.rs"]
mod pgwire_type_declared_null_matrix;
#[path = "support/type_declared_null_matrix.rs"]
mod support_type_declared_null_matrix;

#[path = "pgwire_extended/protocol/boolean_contexts.rs"]
mod pgwire_boolean_contexts;

#[path = "pgwire_extended/protocol/boolean_binary.rs"]
mod pgwire_boolean_binary;

#[path = "pgwire_extended/protocol/scalar_receive_errors.rs"]
mod pgwire_scalar_receive_errors;
#[path = "support/scalar_receive.rs"]
mod support_scalar_receive;

#[path = "pgwire_extended/aggregates/numeric/numeric_metadata.rs"]
mod pgwire_type_numeric_metadata;
#[path = "pgwire_extended/type_resolution/output_source_seams.rs"]
mod pgwire_type_output_source_seams;

#[path = "pgwire_extended/type_resolution/output_identifier_identity.rs"]
mod pgwire_type_output_identifier_identity;

#[path = "pgwire_extended/ctes/materialized_projection_metadata.rs"]
mod pgwire_materialized_projection_metadata;

// Formerly tests/pgwire_extended_control.rs.
#[path = "pgwire_extended/protocol/control.rs"]
mod pgwire_extended_control;

// Formerly tests/pgwire_extended_execution.rs.
#[path = "pgwire_extended/protocol/execution.rs"]
mod pgwire_extended_execution;
// Formerly tests/pgwire_extended_lifecycle.rs.
#[path = "pgwire_extended/protocol/lifecycle.rs"]
mod pgwire_extended_lifecycle;

// Formerly tests/pgwire_extended_metadata.rs.
#[path = "pgwire_extended/descriptors/metadata.rs"]
mod pgwire_extended_metadata;

// Formerly tests/pgwire_extended_prepared.rs.
#[path = "pgwire_extended/prepared_statements/prepared.rs"]
mod pgwire_extended_prepared;

// Formerly tests/pgwire_portal_safety.rs.
#[path = "pgwire_extended/portals/portal_safety.rs"]
mod pgwire_portal_safety;

// Formerly tests/pgwire_portal_streaming.rs.
#[path = "pgwire_extended/portals/portal_streaming.rs"]
mod pgwire_portal_streaming;

// Formerly tests/pgwire_simple_query_batch.rs.
#[path = "pgwire_extended/simple_query/simple_query_batch.rs"]
mod pgwire_simple_query_batch;

#[path = "pgwire_extended/portals/portal_completion.rs"]
mod pgwire_portal_completion;

// DataRow framing must agree with the RowDescription sent for the same result.
#[path = "pgwire_extended/results/result_framing.rs"]
mod pgwire_result_framing;

#[path = "pgwire_extended/portals/row_limited_portal.rs"]
mod pgwire_row_limited_portal;

#[path = "pgwire_extended/empty_query/semantics.rs"]
mod pgwire_empty_query;

// Declared bind parameter types checked against BOOLEAN operands.
#[path = "pgwire_extended/parameters/boolean_parameter_families.rs"]
mod pgwire_boolean_parameter_families;
#[path = "pgwire_extended/transactions/transaction_boundaries.rs"]
mod pgwire_transaction_boundaries;
#[path = "pgwire_extended/transactions/transaction_portal_handoffs.rs"]
mod pgwire_transaction_portal_handoffs;

#[path = "pgwire_extended/comparisons/null_safe/null_safe_predicates.rs"]
mod null_safe_predicates;

#[path = "support/collection_aggregate_resources.rs"]
mod support_collection_aggregate_resources;

#[path = "support/scalar_presentation.rs"]
mod support_scalar_presentation;

#[path = "support/primitive_literal_inputs.rs"]
mod support_primitive_literal_inputs;

#[path = "support/bind_planning.rs"]
mod support_bind_planning;

#[path = "support/unary_numeric.rs"]
mod support_unary_numeric;

#[path = "support/pagination_unary.rs"]
mod support_pagination_unary;

#[path = "support/fetch.rs"]
mod support_fetch;

#[path = "pgwire_extended/protocol/set_quantifiers.rs"]
mod set_quantifiers;

#[path = "support/values_casts.rs"]
mod support_values_casts;

#[path = "pgwire_extended/grammar/values_casts_date.rs"]
mod values_casts_date;

#[path = "pgwire_extended/grammar/values_casts_time.rs"]
mod values_casts_time;

#[path = "pgwire_extended/grammar/values_casts_timestamp.rs"]
mod values_casts_timestamp;

#[path = "pgwire_extended/grammar/values_casts_uuid.rs"]
mod values_casts_uuid;

#[path = "pgwire_extended/grammar/values_casts_bytea.rs"]
mod values_casts_bytea;

#[path = "pgwire_extended/grammar/values_casts_char.rs"]
mod values_casts_char;

#[path = "pgwire_extended/grammar/values_casts_varchar.rs"]
mod values_casts_varchar;

#[path = "pgwire_extended/grammar/values_casts_parameters.rs"]
mod values_casts_parameters;

#[path = "pgwire_extended/grammar/values_casts_character.rs"]
mod values_casts_character;

#[path = "pgwire_extended/grammar/values_casts_semantics.rs"]
mod values_casts_semantics;

#[path = "pgwire_extended/grammar/values_casts_cte.rs"]
mod values_casts_cte;

#[path = "pgwire_extended/grammar/varchar_scalar.rs"]
mod varchar_scalar;

#[path = "pgwire_extended/grammar/varchar_values.rs"]
mod varchar_values;

#[path = "pgwire_extended/grammar/varchar_derived.rs"]
mod varchar_derived;

#[path = "pgwire_extended/grammar/varchar_cte.rs"]
mod varchar_cte;

#[path = "pgwire_extended/grammar/varchar_set.rs"]
mod varchar_set;

#[path = "pgwire_extended/grammar/varchar_parameters.rs"]
mod varchar_parameters;

#[path = "pgwire_extended/types/bit_bytea/casts_int2.rs"]
mod bytea_int2;

#[path = "pgwire_extended/types/bit_bytea/casts_int4.rs"]
mod bytea_int4;

#[path = "pgwire_extended/types/bit_bytea/casts_int8.rs"]
mod bytea_int8;

#[path = "pgwire_extended/types/bit_bytea/decode_int2.rs"]
mod bytea_decode_int2;

#[path = "pgwire_extended/types/bit_bytea/decode_int4.rs"]
mod bytea_decode_int4;

#[path = "pgwire_extended/types/bit_bytea/decode_int8.rs"]
mod bytea_decode_int8;

#[path = "pgwire_extended/types/bit_bytea/cast_errors.rs"]
mod bytea_cast_errors;

#[path = "pgwire_extended/types/bit_bytea/cast_parameters.rs"]
mod bytea_cast_parameters;

#[path = "pgwire_extended/types/bit_bytea/cast_mixed.rs"]
mod bytea_cast_mixed;

#[path = "pgwire_extended/types/bit_bytea/cast_parameter_errors.rs"]
mod bytea_cast_parameter_errors;

#[path = "pgwire_extended/types/bit_bytea/cast_scopes.rs"]
mod bytea_cast_scopes;

#[path = "pgwire_extended/types/bit_bytea/cast_lazy_parameters.rs"]
mod bytea_cast_lazy_parameters;

#[path = "support/bytea_cast_planning.rs"]
mod support_bytea_cast_planning;

#[path = "support/bytea_parameter_recovery.rs"]
mod support_bytea_parameter_recovery;
