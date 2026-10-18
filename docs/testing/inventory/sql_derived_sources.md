# sql_derived_sources test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `should_apply_cte_alias_lists_over_wildcard_bodies` | [tests/sql_derived_sources/ctes/cte_alias_lists.rs:6](../../../tests/sql_derived_sources/ctes/cte_alias_lists.rs#L6) | `tests/sql_derived_sources/ctes/cte_alias_lists.rs` | registered | no | 1 | selected |
| `should_type_renamed_derived_columns_by_their_own_source` | [tests/sql_derived_sources/predicates/derived_predicate_typing.rs:20](../../../tests/sql_derived_sources/predicates/derived_predicate_typing.rs#L20) | `tests/sql_derived_sources/predicates/derived_predicate_typing.rs` | registered | no | 1 | selected |
| `should_canonicalize_typed_literals_over_a_cte` | [tests/sql_derived_sources/predicates/derived_predicate_typing.rs:37](../../../tests/sql_derived_sources/predicates/derived_predicate_typing.rs#L37) | `tests/sql_derived_sources/predicates/derived_predicate_typing.rs` | registered | no | 1 | selected |
| `should_validate_operand_families_over_a_cte` | [tests/sql_derived_sources/predicates/derived_predicate_typing.rs:63](../../../tests/sql_derived_sources/predicates/derived_predicate_typing.rs#L63) | `tests/sql_derived_sources/predicates/derived_predicate_typing.rs` | registered | no | 1 | selected |
| `should_apply_fulltext_index_options_through_derived_sources` | [tests/sql_derived_sources/semantics/derived_fulltext_options.rs:6](../../../tests/sql_derived_sources/semantics/derived_fulltext_options.rs#L6) | `tests/sql_derived_sources/semantics/derived_fulltext_options.rs` | registered | no | 1 | selected |
| `should_describe_derived_table_columns_with_their_real_types` | [tests/sql_derived_sources/semantics/derived_result_metadata.rs:34](../../../tests/sql_derived_sources/semantics/derived_result_metadata.rs#L34) | `tests/sql_derived_sources/semantics/derived_result_metadata.rs` | registered | no | 1 | selected |
| `should_not_adopt_a_same_named_table_schema_for_an_alias` | [tests/sql_derived_sources/semantics/derived_result_metadata.rs:62](../../../tests/sql_derived_sources/semantics/derived_result_metadata.rs#L62) | `tests/sql_derived_sources/semantics/derived_result_metadata.rs` | registered | no | 1 | selected |
