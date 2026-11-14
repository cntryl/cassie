# sql_contextual_literals test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_reject_invalid_json_string_comparands` | [tests/sql_contextual_literals/contracts/errors.rs:2](../../../tests/sql_contextual_literals/contracts/errors.rs#L2) | `tests/sql_contextual_literals/contracts/errors.rs` | registered | no | 1 | selected |
| `should_compare_jsonb_column_with_json_string_literal` | [tests/sql_contextual_literals/contracts/semantics.rs:2](../../../tests/sql_contextual_literals/contracts/semantics.rs#L2) | `tests/sql_contextual_literals/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_compare_jsonb_scalar_string_literals` | [tests/sql_contextual_literals/contracts/semantics.rs:76](../../../tests/sql_contextual_literals/contracts/semantics.rs#L76) | `tests/sql_contextual_literals/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_compare_text_with_array_shaped_string_literal` | [tests/sql_contextual_literals/contracts/semantics.rs:165](../../../tests/sql_contextual_literals/contracts/semantics.rs#L165) | `tests/sql_contextual_literals/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_compare_typed_columns_with_quoted_literals` | [tests/sql_contextual_literals/contracts/semantics.rs:241](../../../tests/sql_contextual_literals/contracts/semantics.rs#L241) | `tests/sql_contextual_literals/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_apply_quoted_literal_predicates_when_updating_and_deleting` | [tests/sql_contextual_literals/contracts/semantics.rs:276](../../../tests/sql_contextual_literals/contracts/semantics.rs#L276) | `tests/sql_contextual_literals/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_reject_a_quoted_literal_that_is_not_valid_for_the_compared_column` | [tests/sql_contextual_literals/contracts/semantics.rs:306](../../../tests/sql_contextual_literals/contracts/semantics.rs#L306) | `tests/sql_contextual_literals/contracts/semantics.rs` | registered | no | 1 | selected |
