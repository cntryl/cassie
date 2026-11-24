// Integration suite: derived tables and CTEs as FROM sources — their column
// names, column types and predicate typing must match the base table they
// read from.

use cassie_test_support::support_sql_fixture;

#[path = "sql_derived_sources/semantics/derived_result_metadata.rs"]
mod derived_result_metadata;

#[path = "sql_derived_sources/predicates/derived_predicate_typing.rs"]
mod derived_predicate_typing;

#[path = "sql_derived_sources/ctes/cte_alias_lists.rs"]
mod cte_alias_lists;

#[path = "sql_derived_sources/semantics/derived_fulltext_options.rs"]
mod derived_fulltext_options;
