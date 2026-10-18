// Integration suite: set operations (UNION, INTERSECT, EXCEPT) and the
// statements that embed them.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

#[path = "sql_set_operations/set_operations/set_operation_views.rs"]
mod set_operation_views;

#[path = "sql_set_operations/set_operations/set_operation_row_caps.rs"]
mod set_operation_row_caps;

#[path = "sql_set_operations/set_operations/set_operation_chains.rs"]
mod set_operation_chains;

#[path = "sql_set_operations/set_operations/set_operation_widths.rs"]
mod set_operation_widths;
