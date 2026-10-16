#[path = "aggregates/aggregate_query_contracts.rs"]
mod postgresql18_aggregate_query_gaps;
#[path = "aggregates/semantic_edges.rs"]
mod postgresql18_aggregate_semantic_edges;
#[path = "types/bit_bytea/binary_bytea.rs"]
mod postgresql18_binary_bytea;
#[path = "ctes/ctes.rs"]
mod postgresql18_ctes;
#[path = "types/geometry/geometry.rs"]
mod postgresql18_geometry;
#[path = "expressions/inheritance_dml/semantics.rs"]
mod postgresql18_inheritance_dml_wave_g;
#[path = "inheritance/query_metadata.rs"]
mod postgresql18_inheritance_query_wave_h;
#[path = "aggregates/grouping/math_functions.rs"]
mod postgresql18_math_functions;
#[path = "aggregates/money/money.rs"]
mod postgresql18_money;
#[path = "expressions/mutations.rs"]
mod postgresql18_mutations;
#[path = "types/network/network_types.rs"]
mod postgresql18_network_types;
#[path = "aggregates/ranges/numeric.rs"]
mod postgresql18_numeric;
#[path = "patterns/pattern_matching.rs"]
mod postgresql18_pattern_matching;
#[path = "functions/string/query/semantics.rs"]
mod postgresql18_query_inventory_wave3;
#[path = "aggregates/ranges/ranges.rs"]
mod postgresql18_ranges;
#[path = "relations/relations.rs"]
mod postgresql18_relations;
#[path = "expressions/rules.rs"]
mod postgresql18_rules;
#[path = "set_operations/set_operations.rs"]
mod postgresql18_set_operations;
#[path = "aggregates/sql_defined/sql_aggregates.rs"]
mod postgresql18_sql_aggregates;
#[path = "casts/sql_casts.rs"]
mod postgresql18_sql_casts;
#[path = "operators/sql_operators.rs"]
mod postgresql18_sql_operators;
#[path = "functions/string/string_functions.rs"]
mod postgresql18_string_functions;
#[path = "table_sampling/tablesample.rs"]
mod postgresql18_tablesample;
#[path = "aggregates/temporal/temporal.rs"]
mod postgresql18_temporal;
#[path = "functions/temporal/temporal_functions.rs"]
mod postgresql18_temporal_functions;
#[path = "text_search/text_search.rs"]
mod postgresql18_text_search;
#[path = "types/uuid_lsn/uuid_hash_lsn.rs"]
mod postgresql18_uuid_hash_lsn;
#[path = "set_returning/values_aggregates.rs"]
mod postgresql18_values_aggregate_inventory_wave2;
#[path = "relations/values_rows.rs"]
mod postgresql18_values_rows;
#[path = "aggregates/xml/xml.rs"]
mod postgresql18_xml;

#[path = "aggregates/sql_defined/ordered_set.rs"]
mod aggregates_sql_defined_ordered_set;

#[path = "aggregates/sql_defined/serialized_state.rs"]
mod aggregates_sql_defined_serialized_state;

#[path = "set_returning/array_subscripts.rs"]
mod set_returning_array_subscripts;

#[path = "set_returning/lateral_ordinality.rs"]
mod set_returning_lateral_ordinality;

#[path = "types/geometry/operator_nulls.rs"]
mod types_geometry_operator_nulls;

#[path = "relations/values_admission.rs"]
mod postgresql18_values_admission;

#[path = "casts/integer_inputs.rs"]
mod postgresql18_integer_inputs;

#[path = "relations/row_predicates.rs"]
mod postgresql18_row_predicates;

#[path = "relations/values_constants.rs"]
mod postgresql18_values_constants;

#[path = "casts/scalar_presentation.rs"]
mod postgresql18_scalar_presentation;

#[path = "casts/exact_numeric_literals.rs"]
mod exact_numeric_literals;
