// Registrations retain the existing pgwire_extended module namespace.

#[path = "merge/merge_row_images.rs"]
mod merge_row_images;

#[path = "aggregates/aggregate_errors.rs"]
mod postgresql18_aggregate_errors;

#[path = "aggregates/arrays/array_aggregate.rs"]
mod postgresql18_array_aggregate;

#[path = "protocol/cte.rs"]
mod postgresql18_cte_compat;

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

#[path = "table_sampling/tablesample_seed.rs"]
mod postgresql18_tablesample_seed;

#[path = "protocol/temporal/semantics.rs"]
mod postgresql18_temporal_compat;

#[path = "functions/temporal/abbreviation_input.rs"]
mod postgresql18_temporal_abbreviation_input;

#[path = "protocol/text.rs"]
mod postgresql18_text_compat;

#[path = "aggregates/windows/window_function_wire.rs"]
mod postgresql18_window_function_wire;

#[path = "subqueries/scalar_subqueries.rs"]
mod scalar_subqueries;
#[path = "set_returning/series_contracts.rs"]
mod series_sources;
#[path = "types/arrays/unnest_sources.rs"]
mod unnest_sources;

#[path = "set_returning/series_contracts.rs"]
mod series_sources;

#[path = "aggregates/ordered_set/arrays.rs"]
mod aggregates_ordered_set_arrays;

#[path = "aggregates/ordered_set/resources.rs"]
mod aggregates_ordered_set_resources;

#[path = "ctes/cancellation.rs"]
mod cte_cycle_recovery;

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

#[path = "aggregates/array_agg/resources.rs"]
mod aggregates_array_agg_resources;

#[path = "aggregates/json_agg/resources.rs"]
mod aggregates_json_agg_resources;

#[path = "aggregates/json_array_conversion.rs"]
mod aggregates_json_array_conversion;

#[path = "aggregates/json_typed_conversion.rs"]
mod aggregates_json_typed_conversion;

#[path = "aggregates/json_object_agg/resources.rs"]
mod aggregates_json_object_agg_resources;

#[path = "aggregates/jsonb_agg/resources.rs"]
mod aggregates_jsonb_agg_resources;

#[path = "aggregates/jsonb_object_agg/resources.rs"]
mod aggregates_jsonb_object_agg_resources;

#[path = "aggregates/string_agg/resources.rs"]
mod aggregates_string_agg_resources;

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

#[path = "ddl/concurrent_table_creation.rs"]
mod ddl_concurrent_table_creation;

#[path = "ddl/ctas_atomicity.rs"]
mod ddl_ctas_atomicity;

#[path = "ddl/explicit_null.rs"]
mod ddl_explicit_null;

#[path = "protocol/fetch_global.rs"]
mod fetch_global;
