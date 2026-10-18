# sql_relation_identifiers test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_preserve_quoted_dotted_relation_identity_after_restart` | [tests/sql_relation_identifiers/contracts/restart.rs:2](../../../tests/sql_relation_identifiers/contracts/restart.rs#L2) | `tests/sql_relation_identifiers/contracts/restart.rs` | registered | no | 1 | selected |
| `should_copy_rows_into_a_quoted_dotted_relation` | [tests/sql_relation_identifiers/contracts/semantics.rs:2](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L2) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_distinguish_quoted_dotted_relation_from_schema_qualified_relation` | [tests/sql_relation_identifiers/contracts/semantics.rs:29](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L29) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_distinguish_scoped_quoted_relation_from_schema_qualified_relation` | [tests/sql_relation_identifiers/contracts/semantics.rs:44](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L44) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_not_resolve_a_missing_quoted_dotted_relation_to_a_schema_qualified_relation` | [tests/sql_relation_identifiers/contracts/semantics.rs:59](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L59) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_preserve_quoted_relation_name_through_ddl_lifecycle` | [tests/sql_relation_identifiers/contracts/semantics.rs:77](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L77) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_preserve_quoted_view_name_through_ddl_lifecycle` | [tests/sql_relation_identifiers/contracts/semantics.rs:113](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L113) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
| `should_preserve_qualified_dotted_field_values_outside_wire_profile` | [tests/sql_relation_identifiers/contracts/semantics.rs:134](../../../tests/sql_relation_identifiers/contracts/semantics.rs#L134) | `tests/sql_relation_identifiers/contracts/semantics.rs` | registered | no | 1 | selected |
