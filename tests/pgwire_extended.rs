#[path = "pgwire_extended/conflicts/unique_validation_errors.rs"]
mod unique_validation_errors;

#[path = "pgwire_extended/types/numeric/mixed_numeric_comparisons.rs"]
mod mixed_numeric_comparisons;
#[path = "support/mixed_numeric.rs"]
mod support_mixed_numeric;

#[path = "pgwire_extended/parameters/cache_identity.rs"]
mod parameter_cache_identity;
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
#[path = "support/parameter_cache_identity.rs"]
mod support_parameter_cache_identity;
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

#[path = "pgwire_extended/types/native_name.rs"]
mod pgwire_native_name;
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

#[path = "pgwire_extended/types/character/bpchar_casts.rs"]
mod pgwire_type_bpchar_casts;
#[path = "pgwire_extended/types/real/float_precision.rs"]
mod pgwire_type_float_precision;
#[path = "pgwire_extended/aggregates/numeric/numeric_metadata.rs"]
mod pgwire_type_numeric_metadata;
#[path = "pgwire_extended/type_resolution/output_source_seams.rs"]
mod pgwire_type_output_source_seams;
#[path = "pgwire_extended/types/real/real_wire.rs"]
mod pgwire_type_real_wire;

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

#[path = "pgwire_extended/descriptors/projection_verification.rs"]
mod projection_verification_describe;

#[path = "pgwire_extended/descriptors/projection_command_families.rs"]
mod projection_command_families_describe;

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

#[path = "pgwire_extended/types/bit_bytea/input_literal.rs"]
mod bytea_input_literal;

#[path = "pgwire_extended/types/bit_bytea/input_text.rs"]
mod bytea_input_text;

#[path = "pgwire_extended/types/bit_bytea/input_cte.rs"]
mod bytea_input_cte;

#[path = "pgwire_extended/types/bit_bytea/input_errors_scalar.rs"]
mod bytea_input_errors_scalar;

#[path = "pgwire_extended/types/bit_bytea/input_errors_dead.rs"]
mod bytea_input_errors_dead;

#[path = "pgwire_extended/types/bit_bytea/input_errors_cte.rs"]
mod bytea_input_errors_cte;

#[path = "pgwire_extended/types/bit_bytea/input_errors_text.rs"]
mod bytea_input_errors_text;

#[path = "pgwire_extended/types/bit_bytea/input_parameter_errors.rs"]
mod bytea_input_parameter_errors;

#[path = "support/bytea_input_errors.rs"]
mod support_bytea_input_errors;

#[path = "pgwire_extended/types/bit_bytea/input_parameters.rs"]
mod bytea_input_parameters;

#[path = "pgwire_extended/types/bit_bytea/input_precedence.rs"]
mod bytea_input_precedence;

#[path = "pgwire_extended/types/bit_bytea/input_allbytes.rs"]
mod bytea_input_allbytes;

#[path = "pgwire_extended/catalogs/empty_registries_a.rs"]
mod catalog_empty_registries_a;

#[path = "pgwire_extended/catalogs/empty_registries_b.rs"]
mod catalog_empty_registries_b;

#[path = "pgwire_extended/catalogs/unavailable_virtual_view.rs"]
mod catalog_unavailable_virtual_view;

#[path = "support/catalog_read_model_wire.rs"]
mod support_catalog_read_model_wire;

#[path = "pgwire_extended/catalogs/scalar_0.rs"]
mod catalog_scalar_0;

#[path = "pgwire_extended/catalogs/scalar_1.rs"]
mod catalog_scalar_1;

#[path = "pgwire_extended/catalogs/scalar_2.rs"]
mod catalog_scalar_2;

#[path = "pgwire_extended/catalogs/scalar_3.rs"]
mod catalog_scalar_3;

#[path = "pgwire_extended/catalogs/scalar_4.rs"]
mod catalog_scalar_4;

#[path = "pgwire_extended/catalogs/scalar_errors.rs"]
mod catalog_scalar_errors;

#[path = "pgwire_extended/catalogs/scalar_parameters.rs"]
mod catalog_scalar_parameters;

#[path = "pgwire_extended/catalogs/scalar_signatures.rs"]
mod catalog_scalar_signatures;

#[path = "pgwire_extended/catalogs/scalar_precedence.rs"]
mod catalog_scalar_precedence;

#[path = "pgwire_extended/catalogs/scalar_pending.rs"]
mod catalog_scalar_pending;

#[path = "pgwire_extended/catalogs/runtime_scalars_0.rs"]
mod catalog_runtime_scalars_0;

#[path = "pgwire_extended/catalogs/runtime_scalars_1.rs"]
mod catalog_runtime_scalars_1;

#[path = "pgwire_extended/catalogs/runtime_modules.rs"]
mod catalog_runtime_modules;

#[path = "support/runtime_information_wire.rs"]
mod support_runtime_information_wire;

#[path = "pgwire_extended/catalogs/runtime_parameters.rs"]
mod catalog_runtime_parameters;

#[path = "pgwire_extended/catalogs/runtime_signatures.rs"]
mod catalog_runtime_signatures;

#[path = "pgwire_extended/catalogs/options_0.rs"]
mod catalog_options_0;

#[path = "pgwire_extended/catalogs/options_1.rs"]
mod catalog_options_1;

#[path = "support/options_wire.rs"]
mod support_options_wire;

#[path = "pgwire_extended/catalogs/options_parameters.rs"]
mod catalog_options_parameters;

#[path = "pgwire_extended/catalogs/options_signatures.rs"]
mod catalog_options_signatures;

#[path = "pgwire_extended/catalogs/options_errors.rs"]
mod catalog_options_errors;

#[path = "pgwire_extended/catalogs/options_escaping.rs"]
mod catalog_options_escaping;

#[path = "pgwire_extended/catalogs/current_query.rs"]
mod catalog_current_query;

#[path = "pgwire_extended/catalogs/current_query_simple.rs"]
mod catalog_current_query_simple;

#[path = "pgwire_extended/catalogs/current_query_lifecycle.rs"]
mod catalog_current_query_lifecycle;

#[path = "pgwire_extended/catalogs/current_query_parameters.rs"]
mod catalog_current_query_parameters;

#[path = "pgwire_extended/catalogs/current_query_arity.rs"]
mod catalog_current_query_arity;

#[path = "pgwire_extended/catalogs/datum_sizes_0.rs"]
mod catalog_datum_sizes_0;

#[path = "pgwire_extended/catalogs/datum_sizes_1.rs"]
mod catalog_datum_sizes_1;

#[path = "pgwire_extended/catalogs/datum_sizes_2.rs"]
mod catalog_datum_sizes_2;

#[path = "pgwire_extended/catalogs/datum_sizes_3.rs"]
mod catalog_datum_sizes_3;

#[path = "pgwire_extended/catalogs/datum_parameters.rs"]
mod catalog_datum_parameters;

#[path = "pgwire_extended/catalogs/datum_errors.rs"]
mod catalog_datum_errors;

#[path = "pgwire_extended/catalogs/datum_nested.rs"]
mod catalog_datum_nested;

#[path = "pgwire_extended/grammar/scalar_batch_01.rs"]
mod scalar_batch_01;

#[path = "pgwire_extended/grammar/scalar_batch_01_parameters.rs"]
mod scalar_batch_01_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_02.rs"]
mod scalar_batch_02;

#[path = "pgwire_extended/grammar/scalar_batch_02_errors.rs"]
mod scalar_batch_02_errors;

#[path = "pgwire_extended/grammar/scalar_batch_02_special.rs"]
mod scalar_batch_02_special;

#[path = "pgwire_extended/grammar/scalar_batch_02_pending.rs"]
mod scalar_batch_02_pending;

#[path = "pgwire_extended/grammar/scalar_batch_03.rs"]
mod scalar_batch_03;

#[path = "pgwire_extended/grammar/scalar_batch_03_parameters.rs"]
mod scalar_batch_03_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_03_gap.rs"]
mod scalar_batch_03_gap;

#[path = "pgwire_extended/grammar/scalar_batch_03_unicode.rs"]
mod scalar_batch_03_unicode;

#[path = "pgwire_extended/grammar/scalar_batch_04.rs"]
mod scalar_batch_04;

#[path = "pgwire_extended/grammar/scalar_batch_04_parameters.rs"]
mod scalar_batch_04_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_04_errors.rs"]
mod scalar_batch_04_errors;

#[path = "pgwire_extended/grammar/scalar_batch_04_parameter_errors.rs"]
mod scalar_batch_04_parameter_errors;
#[path = "support/unary_math_parameter_recovery.rs"]
mod support_unary_math_parameter_recovery;

#[path = "pgwire_extended/grammar/scalar_batch_04_interactions.rs"]
mod scalar_batch_04_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_04_prefix.rs"]
mod scalar_batch_04_prefix;

#[path = "pgwire_extended/grammar/scalar_batch_05.rs"]
mod scalar_batch_05;

#[path = "pgwire_extended/grammar/scalar_batch_05_parameters.rs"]
mod scalar_batch_05_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_05_interactions.rs"]
mod scalar_batch_05_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_05_errors.rs"]
mod scalar_batch_05_errors;

#[path = "pgwire_extended/grammar/scalar_batch_05_null_parameters.rs"]
mod scalar_batch_05_null_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_06.rs"]
mod scalar_batch_06;

#[path = "pgwire_extended/grammar/scalar_batch_06_parameters.rs"]
mod scalar_batch_06_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_06_interactions.rs"]
mod scalar_batch_06_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_06_index.rs"]
mod scalar_batch_06_index;

#[path = "pgwire_extended/grammar/scalar_batch_07.rs"]
mod scalar_batch_07;

#[path = "pgwire_extended/grammar/scalar_batch_07_interactions.rs"]
mod scalar_batch_07_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_07_errors.rs"]
mod scalar_batch_07_errors;

#[path = "pgwire_extended/grammar/scalar_batch_07_parameters.rs"]
mod scalar_batch_07_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_07_routines.rs"]
mod scalar_batch_07_routines;

#[path = "pgwire_extended/grammar/scalar_batch_08.rs"]
mod scalar_batch_08;

#[path = "pgwire_extended/grammar/scalar_batch_08_interactions.rs"]
mod scalar_batch_08_interactions;

#[path = "pgwire_extended/grammar/scalar_batch_08_errors.rs"]
mod scalar_batch_08_errors;

#[path = "pgwire_extended/grammar/scalar_batch_08_parameters.rs"]
mod scalar_batch_08_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_09.rs"]
mod scalar_batch_09;

#[path = "pgwire_extended/grammar/scalar_batch_09_catalog.rs"]
mod scalar_batch_09_catalog;

#[path = "pgwire_extended/grammar/scalar_batch_09_scopes.rs"]
mod scalar_batch_09_scopes;

#[path = "pgwire_extended/grammar/scalar_batch_09_parameters.rs"]
mod scalar_batch_09_parameters;

#[path = "pgwire_extended/grammar/scalar_batch_10.rs"]
mod scalar_batch_10;

#[path = "pgwire_extended/grammar/scalar_batch_10_errors.rs"]
mod scalar_batch_10_errors;

#[path = "pgwire_extended/grammar/scalar_batch_10_arithmetic_scopes.rs"]
mod scalar_batch_10_arithmetic_scopes;

#[path = "pgwire_extended/expressions/conditional/type_diagnostics.rs"]
mod conditional_type_diagnostics;

#[path = "pgwire_extended/grammar/qualified_operators_0.rs"]
mod qualified_operators_0;

#[path = "pgwire_extended/grammar/qualified_operators_1.rs"]
mod qualified_operators_1;

#[path = "pgwire_extended/grammar/qualified_operators_mixed.rs"]
mod qualified_operators_mixed;

#[path = "pgwire_extended/grammar/qualified_operators_bytea.rs"]
mod qualified_operators_bytea;

#[path = "pgwire_extended/grammar/qualified_operators_parameters.rs"]
mod qualified_operators_parameters;

#[path = "pgwire_extended/grammar/qualified_operators_errors.rs"]
mod qualified_operators_errors;

#[path = "pgwire_extended/grammar/bytea_functions_values_0.rs"]
mod bytea_functions_values_0;

#[path = "pgwire_extended/grammar/bytea_functions_values_1.rs"]
mod bytea_functions_values_1;

#[path = "pgwire_extended/grammar/bytea_functions_values_2.rs"]
mod bytea_functions_values_2;

#[path = "pgwire_extended/grammar/bytea_functions_values_3.rs"]
mod bytea_functions_values_3;

#[path = "pgwire_extended/grammar/bytea_functions_values_4.rs"]
mod bytea_functions_values_4;

#[path = "pgwire_extended/grammar/bytea_functions_errors_0.rs"]
mod bytea_functions_errors_0;

#[path = "pgwire_extended/grammar/bytea_functions_errors_1.rs"]
mod bytea_functions_errors_1;

#[path = "pgwire_extended/grammar/bytea_functions_errors_2.rs"]
mod bytea_functions_errors_2;

#[path = "pgwire_extended/grammar/bytea_functions_parameters.rs"]
mod bytea_functions_parameters;

#[path = "pgwire_extended/grammar/bytea_functions_empty.rs"]
mod bytea_functions_empty;

#[path = "pgwire_extended/grammar/bytea_functions_gaps.rs"]
mod bytea_functions_gaps;

#[path = "pgwire_extended/grammar/bytea_functions_overloads.rs"]
mod bytea_functions_overloads;

#[path = "support/parameter_operators.rs"]
mod support_parameter_operators;

#[path = "pgwire_extended/parameters/operator_resolution.rs"]
mod parameter_operator_resolution;

#[path = "pgwire_extended/types/review_diagnostics.rs"]
mod review_diagnostics;
