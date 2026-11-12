// pgwire_extended: subsystem-owned wire contracts; shared fixtures stay in support/.

#[path = "pgwire_extended/transactions/repeatable_own_views.rs"]
mod repeatable_own_views;

#[path = "pgwire_extended/transactions/transaction_owned_sequences.rs"]
mod transaction_owned_sequences;

#[path = "pgwire_extended/transactions/repeatable_read.rs"]
mod repeatable_read;

#[path = "pgwire_extended/conflicts/unique_validation_errors.rs"]
mod unique_validation_errors;

#[path = "pgwire_extended/parameters/cache_identity.rs"]
mod parameter_cache_identity;

#[path = "pgwire_extended/parameters/occurrences.rs"]
mod pgwire_parameter_occurrences;

#[path = "pgwire_extended/parameters/parameter_slot_completeness.rs"]
mod pgwire_parameter_slot_completeness;

#[path = "pgwire_extended/parameters/unsigned_parameter_oids.rs"]
mod pgwire_unsigned_parameter_oids;

#[path = "pgwire_extended/parameters/parameter_signatures.rs"]
mod pgwire_parameter_signatures;

#[path = "pgwire_extended/parameters/parameter_admission.rs"]
mod parameter_admission;

#[path = "pgwire_extended/security/portal_authorization.rs"]
mod portal_authorization;

#[path = "pgwire_extended/protocol/statement_visibility.rs"]
mod statement_visibility;

#[path = "pgwire_extended/expressions/conditional/conditional_expressions.rs"]
mod conditional_expressions;

#[path = "pgwire_extended/protocol/constant_planning.rs"]
mod constant_planning;

#[path = "pgwire_extended/expressions/conditional/coalesce_promotion.rs"]
mod coalesce_promotion;

#[path = "pgwire_extended/transactions/transaction_errors.rs"]
mod transaction_errors;

#[path = "pgwire_extended/protocol/vector_carriers.rs"]
mod pgwire_type_vector_carriers;

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

#[path = "pgwire_extended/protocol/boolean_contexts.rs"]
mod pgwire_boolean_contexts;

#[path = "pgwire_extended/protocol/boolean_binary.rs"]
mod pgwire_boolean_binary;

#[path = "pgwire_extended/protocol/scalar_receive_errors.rs"]
mod pgwire_scalar_receive_errors;

#[path = "pgwire_extended/aggregates/numeric/numeric_metadata.rs"]
mod pgwire_type_numeric_metadata;

#[path = "pgwire_extended/ctes/materialized_projection_metadata.rs"]
mod pgwire_materialized_projection_metadata;

#[path = "pgwire_extended/protocol/control.rs"]
mod pgwire_extended_control;

#[path = "pgwire_extended/protocol/execution.rs"]
mod pgwire_extended_execution;

#[path = "pgwire_extended/protocol/lifecycle.rs"]
mod pgwire_extended_lifecycle;

#[path = "pgwire_extended/descriptors/metadata.rs"]
mod pgwire_extended_metadata;

#[path = "pgwire_extended/descriptors/projection_verification.rs"]
mod projection_verification_describe;

#[path = "pgwire_extended/descriptors/projection_command_families.rs"]
mod projection_command_families_describe;

#[path = "pgwire_extended/prepared_statements/prepared.rs"]
mod pgwire_extended_prepared;

#[path = "pgwire_extended/portals/portal_safety.rs"]
mod pgwire_portal_safety;

#[path = "pgwire_extended/portals/portal_streaming.rs"]
mod pgwire_portal_streaming;

#[path = "pgwire_extended/simple_query/simple_query_batch.rs"]
mod pgwire_simple_query_batch;

#[path = "pgwire_extended/portals/portal_completion.rs"]
mod pgwire_portal_completion;

#[path = "pgwire_extended/results/result_framing.rs"]
mod pgwire_result_framing;

#[path = "pgwire_extended/portals/row_limited_portal.rs"]
mod pgwire_row_limited_portal;

#[path = "pgwire_extended/empty_query/semantics.rs"]
mod pgwire_empty_query;

#[path = "pgwire_extended/parameters/boolean_parameter_families.rs"]
mod pgwire_boolean_parameter_families;

#[path = "pgwire_extended/transactions/transaction_boundaries.rs"]
mod pgwire_transaction_boundaries;

#[path = "pgwire_extended/transactions/transaction_portal_handoffs.rs"]
mod pgwire_transaction_portal_handoffs;

#[path = "pgwire_extended/comparisons/null_safe/null_safe_predicates.rs"]
mod null_safe_predicates;

#[path = "pgwire_extended/protocol/set_quantifiers.rs"]
mod set_quantifiers;

#[path = "pgwire_extended/expressions/conditional/type_diagnostics.rs"]
mod conditional_type_diagnostics;

#[path = "pgwire_extended/parameters/operator_resolution.rs"]
mod parameter_operator_resolution;

#[path = "pgwire_extended/aggregates/windows/statistical_filter.rs"]
mod statistical_filter_windows;

use cassie_test_support::support_bind_planning;

#[path = "support/boolean_aggregate_cancellation.rs"]
mod support_boolean_aggregate_cancellation;

#[path = "support/collection_aggregate_resources.rs"]
mod support_collection_aggregate_resources;

#[path = "support/fetch.rs"]
mod support_fetch;

#[path = "support/pagination_unary.rs"]
mod support_pagination_unary;

#[path = "support/parameter_cache_identity.rs"]
mod support_parameter_cache_identity;

use cassie_test_support::support_parameter_occurrence_wire;

#[path = "support/parameter_operators.rs"]
mod support_parameter_operators;

#[path = "support/parameter_signature_wire.rs"]
mod support_parameter_signature_wire;

use cassie_test_support::support_pgwire;

#[path = "support/pgwire_transaction_boundary.rs"]
mod support_pgwire_transaction_boundary;

#[path = "support/portal_authorization.rs"]
mod support_portal_authorization;

#[path = "support/portal_memory_calibration.rs"]
mod support_portal_memory_calibration;

#[path = "support/primitive_literal_inputs.rs"]
mod support_primitive_literal_inputs;

#[path = "support/repeatable_read.rs"]
mod support_repeatable_read;

#[path = "support/scalar_presentation.rs"]
mod support_scalar_presentation;

#[path = "support/scalar_receive.rs"]
mod support_scalar_receive;

#[path = "support/skipped_conflict_returning_wire.rs"]
mod support_skipped_conflict_returning_wire;

use cassie_test_support::support_sql;

use cassie_test_support::support_sql_fixture;

use cassie_test_support::support_temp_dirs;

use cassie_test_support::support_type_codec_matrix;

use cassie_test_support::support_type_metadata_contract;

#[path = "support/typed_portal.rs"]
mod support_typed_portal;

use cassie_test_support::support_unary_numeric;

#[path = "pgwire_extended/mod.rs"]
mod pgwire_extended;
