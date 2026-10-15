# expression_index_upsert test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_resolve_expression_index_conflicts_without_scanning_rows_per_input_row` | [tests/expression_index_upsert.rs:71](../../../tests/expression_index_upsert.rs#L71) | `tests/expression_index_upsert/conflicts/semantics.rs` | registered | no | 1 | required |
| `should_ignore_expression_reservation_owners_deleted_or_rekeyed_in_the_current_transaction` | [tests/expression_index_upsert.rs:108](../../../tests/expression_index_upsert.rs#L108) | `tests/expression_index_upsert/transactions/transactions.rs` | registered | no | 1 | required |
| `should_check_staged_expression_reservation_owner_in_the_current_transaction` | [tests/expression_index_upsert.rs:179](../../../tests/expression_index_upsert.rs#L179) | `tests/expression_index_upsert/transactions/transactions.rs` | registered | no | 1 | required |
| `should_match_a_unique_conflict_index_with_its_explicit_operator_class` | [tests/expression_index_upsert.rs:213](../../../tests/expression_index_upsert.rs#L213) | `tests/expression_index_upsert/operators/semantics.rs` | registered | no | 1 | required |
