# sql_contextual_literals test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_reject_invalid_json_string_comparands` | [tests/sql_contextual_literals.rs:13](../../../tests/sql_contextual_literals.rs#L13) | `tests/sql_contextual_literals/types/json/errors.rs` | registered | no | 1 | required |
| `should_compare_jsonb_column_with_json_string_literal` | [tests/sql_contextual_literals.rs:46](../../../tests/sql_contextual_literals.rs#L46) | `tests/sql_contextual_literals/types/json/semantics.rs` | registered | no | 1 | required |
| `should_compare_jsonb_scalar_string_literals` | [tests/sql_contextual_literals.rs:123](../../../tests/sql_contextual_literals.rs#L123) | `tests/sql_contextual_literals/types/json/semantics.rs` | registered | no | 1 | required |
| `should_compare_text_with_array_shaped_string_literal` | [tests/sql_contextual_literals.rs:215](../../../tests/sql_contextual_literals.rs#L215) | `tests/sql_contextual_literals/types/arrays/semantics.rs` | registered | no | 1 | required |
