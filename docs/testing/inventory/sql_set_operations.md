# sql_set_operations test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `set_operation_views::should_keep_every_set_operation_branch_in_a_view_definition` | [tests/sql_set_operations.rs:43](../../../tests/sql_set_operations.rs#L43) | `tests/sql_set_operations/set_operations/set_operation_views/semantics.rs` | registered | no | 1 | required |
| `set_operation_views::should_keep_the_with_clause_in_a_view_definition` | [tests/sql_set_operations.rs:66](../../../tests/sql_set_operations.rs#L66) | `tests/sql_set_operations/set_operations/set_operation_views/semantics.rs` | registered | no | 1 | required |
| `set_operation_row_caps::should_materialize_the_whole_right_branch_under_a_result_row_cap` | [tests/sql_set_operations.rs:124](../../../tests/sql_set_operations.rs#L124) | `tests/sql_set_operations/ctes/set_operation_row_caps/semantics.rs` | registered | no | 1 | required |
| `set_operation_chains::should_evaluate_mixed_set_operators_left_to_right_with_intersect_first` | [tests/sql_set_operations.rs:198](../../../tests/sql_set_operations.rs#L198) | `tests/sql_set_operations/operators/set_operation_chains/semantics.rs` | registered | no | 1 | required |
| `set_operation_chains::should_accept_intersect_or_except_after_the_first_set_operator` | [tests/sql_set_operations.rs:234](../../../tests/sql_set_operations.rs#L234) | `tests/sql_set_operations/operators/set_operation_chains/semantics.rs` | registered | no | 1 | required |
| `set_operation_chains::should_resolve_global_clauses_over_a_regrouped_set_chain` | [tests/sql_set_operations.rs:259](../../../tests/sql_set_operations.rs#L259) | `tests/sql_set_operations/set_operations/set_operation_chains/semantics.rs` | registered | no | 1 | required |
| `set_operation_widths::should_reject_mismatched_set_widths_when_a_branch_is_empty` | [tests/sql_set_operations.rs:325](../../../tests/sql_set_operations.rs#L325) | `tests/sql_set_operations/set_operations/set_operation_widths/errors.rs` | registered | no | 1 | required |
| `set_operation_widths::should_accept_matching_wildcard_set_widths` | [tests/sql_set_operations.rs:354](../../../tests/sql_set_operations.rs#L354) | `tests/sql_set_operations/set_operations/set_operation_widths/semantics.rs` | registered | no | 1 | required |
