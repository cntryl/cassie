// Integration suite: scalar, row, membership, EXISTS, and NOT EXISTS subqueries.

use cassie_test_support::support_sql_fixture;

#[path = "sql_exists_subqueries/boolean_contexts.rs"]
mod sql_boolean_exists;
#[path = "sql_exists_subqueries/mod.rs"]
mod sql_exists_subqueries;
#[path = "sql_exists_subqueries/target_elision.rs"]
mod sql_exists_target_elision;

#[path = "sql_exists_subqueries/subqueries/exists_in_dml.rs"]
mod exists_in_dml;

#[path = "sql_exists_subqueries/subqueries/exists_outside_where.rs"]
mod exists_outside_where;

#[path = "sql_exists_subqueries/subqueries/exists_cte_scope.rs"]
mod exists_cte_scope;

#[path = "sql_exists_subqueries/subqueries/correlated_exists.rs"]
mod correlated_exists;
