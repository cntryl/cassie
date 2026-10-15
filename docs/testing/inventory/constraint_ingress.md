# constraint_ingress test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_apply_defaults_for_omitted_values_across_supported_ingress` | [tests/constraint_ingress.rs:10](../../../tests/constraint_ingress.rs#L10) | `tests/constraint_ingress/relations/semantics.rs` | registered | no | 1 | required |
| `should_preserve_explicit_values_when_defaults_exist_across_ingress` | [tests/constraint_ingress.rs:74](../../../tests/constraint_ingress.rs#L74) | `tests/constraint_ingress/relations/semantics.rs` | registered | no | 1 | required |
| `should_reject_explicit_null_across_supported_ingress` | [tests/constraint_ingress.rs:116](../../../tests/constraint_ingress.rs#L116) | `tests/constraint_ingress/semantics/errors.rs` | registered | no | 1 | required |
