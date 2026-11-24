// Integration suite: set operations (UNION, INTERSECT, EXCEPT) and the
// statements that embed them.

use cassie_test_support::support_sql;
use cassie_test_support::support_sql_fixture;

#[path = "sql_set_operations/set_operations/set_operation_views.rs"]
mod set_operation_views;

#[path = "sql_set_operations/set_operations/set_operation_row_caps.rs"]
mod set_operation_row_caps;

#[path = "sql_set_operations/set_operations/set_operation_chains.rs"]
mod set_operation_chains;

#[path = "sql_set_operations/set_operations/set_operation_widths.rs"]
mod set_operation_widths;
