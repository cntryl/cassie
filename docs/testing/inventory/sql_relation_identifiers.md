# sql_relation_identifiers test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_preserve_quoted_dotted_relation_identity_after_restart` | [tests/sql_relation_identifiers.rs:11](../../../tests/sql_relation_identifiers.rs#L11) | `tests/sql_relation_identifiers/sequences/restart.rs` | registered | no | 1 | required |
| `should_copy_rows_into_a_quoted_dotted_relation` | [tests/sql_relation_identifiers.rs:92](../../../tests/sql_relation_identifiers.rs#L92) | `tests/sql_relation_identifiers/copy/semantics.rs` | registered | no | 1 | required |
| `should_distinguish_quoted_dotted_relation_from_schema_qualified_relation` | [tests/sql_relation_identifiers.rs:119](../../../tests/sql_relation_identifiers.rs#L119) | `tests/sql_relation_identifiers/ddl/semantics.rs` | registered | no | 1 | required |
| `should_distinguish_scoped_quoted_relation_from_schema_qualified_relation` | [tests/sql_relation_identifiers.rs:134](../../../tests/sql_relation_identifiers.rs#L134) | `tests/sql_relation_identifiers/ddl/semantics.rs` | registered | no | 1 | required |
| `should_not_resolve_a_missing_quoted_dotted_relation_to_a_schema_qualified_relation` | [tests/sql_relation_identifiers.rs:149](../../../tests/sql_relation_identifiers.rs#L149) | `tests/sql_relation_identifiers/ddl/semantics.rs` | registered | no | 1 | required |
| `should_preserve_quoted_relation_name_through_ddl_lifecycle` | [tests/sql_relation_identifiers.rs:167](../../../tests/sql_relation_identifiers.rs#L167) | `tests/sql_relation_identifiers/ddl/semantics.rs` | registered | no | 1 | required |
| `should_preserve_quoted_view_name_through_ddl_lifecycle` | [tests/sql_relation_identifiers.rs:203](../../../tests/sql_relation_identifiers.rs#L203) | `tests/sql_relation_identifiers/ddl/semantics.rs` | registered | no | 1 | required |
| `should_preserve_qualified_dotted_field_values_outside_wire_profile` | [tests/sql_relation_identifiers.rs:292](../../../tests/sql_relation_identifiers.rs#L292) | `tests/sql_relation_identifiers/relations/semantics.rs` | registered | no | 1 | required |
