# json_writes test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_normalize_json_text_writes` | [tests/json_writes.rs:48](../../../tests/json_writes.rs#L48) | `tests/json_writes/types/json/semantics.rs` | registered | no | 1 | required |
| `should_reject_malformed_json_text_insert` | [tests/json_writes.rs:133](../../../tests/json_writes.rs#L133) | `tests/json_writes/types/json/errors.rs` | registered | no | 1 | required |
| `should_reject_malformed_json_text_update` | [tests/json_writes.rs:151](../../../tests/json_writes.rs#L151) | `tests/json_writes/types/json/errors.rs` | registered | no | 1 | required |
| `should_distinguish_json_document_null_from_sql_null` | [tests/json_writes.rs:168](../../../tests/json_writes.rs#L168) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_allow_json_null_in_required_json_field` | [tests/json_writes.rs:262](../../../tests/json_writes.rs#L262) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_preserve_json_null_in_rest_document` | [tests/json_writes.rs:292](../../../tests/json_writes.rs#L292) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_omit_sql_null_fields_from_raw_documents` | [tests/json_writes.rs:320](../../../tests/json_writes.rs#L320) | `tests/json_writes/semantics/nulls.rs` | registered | no | 1 | required |
| `should_return_json_null_from_insert_returning` | [tests/json_writes.rs:364](../../../tests/json_writes.rs#L364) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_preserve_json_null_in_upsert` | [tests/json_writes.rs:393](../../../tests/json_writes.rs#L393) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_distinguish_null_values_after_update` | [tests/json_writes.rs:429](../../../tests/json_writes.rs#L429) | `tests/json_writes/relations/nulls.rs` | registered | no | 1 | required |
| `should_preserve_json_null_distinction_after_restart` | [tests/json_writes.rs:490](../../../tests/json_writes.rs#L490) | `tests/json_writes/types/json/restart.rs` | registered | no | 1 | required |
| `should_preserve_json_null_in_column_storage_paths` | [tests/json_writes.rs:547](../../../tests/json_writes.rs#L547) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_enforce_uniqueness_for_json_null_but_allow_multiple_sql_nulls` | [tests/json_writes.rs:654](../../../tests/json_writes.rs#L654) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_evaluate_partial_unique_predicates_with_json_null_semantics` | [tests/json_writes.rs:724](../../../tests/json_writes.rs#L724) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_check_json_null_foreign_keys_as_values` | [tests/json_writes.rs:773](../../../tests/json_writes.rs#L773) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_assign_sql_null_when_foreign_key_delete_uses_set_null` | [tests/json_writes.rs:823](../../../tests/json_writes.rs#L823) | `tests/json_writes/constraints/nulls.rs` | registered | no | 1 | required |
| `should_assign_sql_null_when_foreign_key_update_uses_set_null` | [tests/json_writes.rs:877](../../../tests/json_writes.rs#L877) | `tests/json_writes/constraints/nulls.rs` | registered | no | 1 | required |
| `should_cascade_sql_null_when_parent_json_null_becomes_sql_null` | [tests/json_writes.rs:931](../../../tests/json_writes.rs#L931) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_apply_check_constraints_to_json_null_values` | [tests/json_writes.rs:985](../../../tests/json_writes.rs#L985) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
| `should_type_json_null_inside_unique_expression_indexes` | [tests/json_writes.rs:1029](../../../tests/json_writes.rs#L1029) | `tests/json_writes/types/json/nulls.rs` | registered | no | 1 | required |
