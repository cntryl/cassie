//! PostgreSQL 18 aggregate and window contracts that remain outside current focused coverage.

use cassie::types::Value;

use super::super::support_sql_fixture::{self, sql_fixture};

#[path = "ordered_set/aggregate_interval_percentile_array/semantics.rs"]
mod aggregate_interval_percentile_array;
#[path = "aggregate_type_boundaries.rs"]
mod aggregate_type_and_window_boundaries;
#[path = "window_oracle_contracts.rs"]
mod aggregate_window_oracle_residuals;
#[path = "../types/arrays/array_aggregate_distinct_nested/semantics.rs"]
mod array_aggregate_distinct_nested;
#[path = "numeric/bitwise_bigint_boundaries/semantics.rs"]
mod bitwise_bigint_boundaries;
#[path = "grouping/group_by_alias_ordinal/semantics.rs"]
mod grouping_alias_and_ordinal;
#[path = "grouping/grouping_functional_dependency/semantics.rs"]
mod grouping_functional_dependency;
#[path = "numeric/numeric_statistical_result_identity/semantics.rs"]
mod numeric_statistical_result_identity;
#[path = "json/ordered_json_aggregate_modifiers/semantics.rs"]
mod ordered_json_aggregate_modifiers;
#[path = "grouping/single_grouping_set/semantics.rs"]
mod single_grouping_set_semantics;
#[path = "strings/string_agg_bytea/semantics.rs"]
mod string_agg_bytea_contract;
#[path = "json/unique_object_aggregate_duplicate_keys/errors.rs"]
mod unique_object_aggregate_duplicate_keys;
#[path = "unique_object_filter_nulls.rs"]
mod unique_object_aggregate_filter_null_contracts;
#[path = "windows/window_ranking_function/semantics.rs"]
mod window_ranking_function_residuals;

fn grouping_fixture(label: &str) -> support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_804_group_values (region TEXT, product TEXT, amount INT, label TEXT, id INT PRIMARY KEY)",
            "INSERT INTO issue_804_group_values VALUES ('east','book',2,'e',1),('east','pen',3,'e',2),('west','book',5,'w',3)",
        ],
    )
}

include!("core/aggregate_query/nulls.rs");

include!("core/aggregate_query/semantics.rs");

include!("boolean/aggregate_query/semantics.rs");

include!("bit_bytea/aggregate_query/semantics.rs");

include!("boolean/aggregate_query/nulls.rs");

include!("ordered_set/aggregate_query/nulls.rs");

include!("ordered_set/aggregate_query/semantics.rs");

include!("windows/aggregate_query/semantics.rs");

include!("../types/numeric/aggregate_query/semantics.rs");

include!("../expressions/aggregate_query/semantics.rs");

include!("statistical/aggregate_query/semantics.rs");

include!("json/aggregate_query/nulls.rs");

include!("strings/aggregate_query/resources.rs");

include!("strings/aggregate_query/semantics.rs");

include!("collections/aggregate_query/nulls.rs");

include!("json/aggregate_query/semantics.rs");
