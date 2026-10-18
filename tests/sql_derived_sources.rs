// Integration suite: derived tables and CTEs as FROM sources — their column
// names, column types and predicate typing must match the base table they
// read from.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "sql_derived_sources/semantics/derived_result_metadata.rs"]
mod derived_result_metadata;

#[path = "sql_derived_sources/predicates/derived_predicate_typing.rs"]
mod derived_predicate_typing;

#[path = "sql_derived_sources/ctes/cte_alias_lists.rs"]
mod cte_alias_lists;

#[path = "sql_derived_sources/semantics/derived_fulltext_options.rs"]
mod derived_fulltext_options;
