#[path = "merge/merge_row_images.rs"]
mod merge_row_images;
#[path = "coordination/advisory_notify.rs"]
mod postgresql18_advisory_notify;
#[path = "aggregates/aggregate_errors.rs"]
mod postgresql18_aggregate_errors;
#[path = "aggregates/arrays/array_aggregate.rs"]
mod postgresql18_array_aggregate;
#[path = "types/arrays/array_boundaries.rs"]
mod postgresql18_array_boundaries;
#[path = "types/bit_bytea/binary_bytea.rs"]
mod postgresql18_binary_bytea_compat;
#[path = "protocol/cte.rs"]
mod postgresql18_cte_compat;
#[path = "protocol/explain_options.rs"]
mod postgresql18_explain_options;
#[path = "grammar.rs"]
mod postgresql18_grammar;
#[path = "functions/math/math.rs"]
mod postgresql18_math_compat;
#[path = "types/network/network.rs"]
mod postgresql18_network_compat;
#[path = "aggregates/numeric/binary.rs"]
mod postgresql18_numeric_compat;
#[path = "query_execution.rs"]
mod postgresql18_query_inventory_wave2;
#[path = "ctes/recursive_cte_cancellation.rs"]
mod postgresql18_recursive_cte_cancellation;
#[path = "subqueries/row_subquery_wire_semantics.rs"]
mod postgresql18_row_subquery_wire_semantics_wave_h;
#[path = "ctes/scalar_types/binary.rs"]
mod postgresql18_scalar_types_inventory_wave3;
#[path = "set_returning/errors.rs"]
mod postgresql18_set_returning_gaps;
#[path = "functions/string_functions.rs"]
mod postgresql18_string_compat;
#[path = "functions/string/string_scalar_edges.rs"]
mod postgresql18_string_scalar_edges;
#[path = "functions/string/string_to_table_limits.rs"]
mod postgresql18_string_to_table_limits;
#[path = "table_sampling/tablesample_seed.rs"]
mod postgresql18_tablesample_seed;
#[path = "protocol/temporal/semantics.rs"]
mod postgresql18_temporal_compat;
#[path = "functions/temporal/temporal_function.rs"]
mod postgresql18_temporal_function_compat;
#[path = "protocol/text.rs"]
mod postgresql18_text_compat;
#[path = "types/uuid_lsn/uuid_hash_lsn.rs"]
mod postgresql18_uuid_hash_lsn_compat;
#[path = "aggregates/windows/window_function_wire.rs"]
mod postgresql18_window_function_wire;
#[path = "routines/record_functions.rs"]
mod record_functions;
#[path = "subqueries/scalar_subqueries.rs"]
mod scalar_subqueries;

#[path = "aggregates/ordered_set/arrays.rs"]
mod aggregates_ordered_set_arrays;
#[path = "aggregates/ordered_set/resources.rs"]
mod aggregates_ordered_set_resources;

#[path = "ctes/cancellation.rs"]
mod cte_cycle_recovery;

#[path = "functions/math/overload_errors.rs"]
mod functions_math_overload_errors;

#[path = "functions/string/error_sqlstates.rs"]
mod functions_string_error_sqlstates;

#[path = "grammar/chained_cast_binary.rs"]
mod grammar_chained_cast_binary;

#[path = "grammar/dollar_quotes.rs"]
mod grammar_dollar_quotes;
#[path = "grammar/lexical_literals.rs"]
mod grammar_lexical_literals;

#[path = "grammar/join_using_errors.rs"]
mod grammar_join_using_errors;

#[path = "grammar/values_errors.rs"]
mod grammar_values_errors;

#[path = "mutations/merge_cancellation.rs"]
mod mutations_merge_cancellation;

#[path = "patterns/escape_errors.rs"]
mod patterns_escape_errors;

#[path = "subqueries/cardinality.rs"]
mod subqueries_cardinality;

#[path = "subqueries/evaluation_count.rs"]
mod subqueries_evaluation_count;

#[path = "subqueries/quantified_rows.rs"]
mod subqueries_quantified_rows;

#[path = "subqueries/resource_limits.rs"]
mod subqueries_resource_limits;

#[path = "subqueries/width.rs"]
mod subqueries_width;

#[path = "type_resolution/polymorphic.rs"]
mod type_resolution_polymorphic;

#[path = "type_resolution/prepared_descriptors.rs"]
mod type_resolution_prepared_descriptors;

#[path = "types/json/jsonpath/cancellation.rs"]
mod types_json_jsonpath_cancellation;

#[path = "types/json/jsonpath_timezone.rs"]
mod types_json_jsonpath_timezone;

#[path = "types/uuid_lsn/restart.rs"]
mod types_uuid_lsn_restart;

#[path = "aggregates/array_agg/resources.rs"]
mod aggregates_array_agg_resources;

#[path = "aggregates/json_agg/resources.rs"]
mod aggregates_json_agg_resources;

#[path = "aggregates/json_object_agg/resources.rs"]
mod aggregates_json_object_agg_resources;

#[path = "aggregates/jsonb_agg/resources.rs"]
mod aggregates_jsonb_agg_resources;

#[path = "aggregates/jsonb_object_agg/resources.rs"]
mod aggregates_jsonb_object_agg_resources;

#[path = "aggregates/string_agg/resources.rs"]
mod aggregates_string_agg_resources;

#[path = "types/network/operator_errors.rs"]
mod types_network_operator_errors;

#[path = "grammar/integer_inputs.rs"]
mod postgresql18_integer_inputs;

#[path = "grammar/row_predicates.rs"]
mod postgresql18_row_predicates;

#[path = "grammar/values_constants.rs"]
mod postgresql18_values_constants;

#[path = "protocol/scalar_presentation.rs"]
mod postgresql18_scalar_presentation;

#[path = "protocol/exact_numeric_literals.rs"]
mod exact_numeric_literals;

#[path = "protocol/primitive_literal_inputs.rs"]
mod primitive_literal_inputs;

#[path = "protocol/parse_analysis.rs"]
mod parse_analysis;

#[path = "protocol/bind_planning.rs"]
mod bind_planning;

#[path = "protocol/cte_reachability.rs"]
mod cte_reachability;

#[path = "grammar/unary_numeric.rs"]
mod unary_numeric;

#[path = "protocol/pagination_unary.rs"]
mod pagination_unary;

#[path = "protocol/pagination_planning.rs"]
mod pagination_planning;

#[path = "protocol/fetch_counts.rs"]
mod fetch_counts;

#[path = "protocol/fetch_offsets.rs"]
mod fetch_offsets;

#[path = "protocol/fetch_contexts.rs"]
mod fetch_contexts;

#[path = "protocol/fetch_errors.rs"]
mod fetch_errors;

#[path = "protocol/fetch_parameters.rs"]
mod fetch_parameters;

#[path = "protocol/fetch_boundaries.rs"]
mod fetch_boundaries;

#[path = "protocol/fetch_names.rs"]
mod fetch_names;

#[path = "protocol/fetch_global.rs"]
mod fetch_global;
