# sql_derived_sources test placements

Generated from `inventory.json`. Destinations are proposed; execution status is not assessed.

| Test | Current source | Proposed file | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `derived_result_metadata::should_describe_derived_table_columns_with_their_real_types` | [tests/sql_derived_sources.rs:46](../../../tests/sql_derived_sources.rs#L46) | `tests/sql_derived_sources/semantics/derived_result_metadata/metadata.rs` | registered | no | 1 | required |
| `derived_result_metadata::should_not_adopt_a_same_named_table_schema_for_an_alias` | [tests/sql_derived_sources.rs:74](../../../tests/sql_derived_sources.rs#L74) | `tests/sql_derived_sources/ddl/derived_result_metadata/semantics.rs` | registered | no | 1 | required |
| `derived_predicate_typing::should_type_renamed_derived_columns_by_their_own_source` | [tests/sql_derived_sources.rs:120](../../../tests/sql_derived_sources.rs#L120) | `tests/sql_derived_sources/predicates/derived_predicate_typing/semantics.rs` | registered | no | 1 | required |
| `derived_predicate_typing::should_canonicalize_typed_literals_over_a_cte` | [tests/sql_derived_sources.rs:137](../../../tests/sql_derived_sources.rs#L137) | `tests/sql_derived_sources/grammar/derived_predicate_typing/semantics.rs` | registered | no | 1 | required |
| `derived_predicate_typing::should_validate_operand_families_over_a_cte` | [tests/sql_derived_sources.rs:164](../../../tests/sql_derived_sources.rs#L164) | `tests/sql_derived_sources/predicates/derived_predicate_typing/semantics.rs` | registered | no | 1 | required |
| `cte_alias_lists::should_apply_cte_alias_lists_over_wildcard_bodies` | [tests/sql_derived_sources.rs:202](../../../tests/sql_derived_sources.rs#L202) | `tests/sql_derived_sources/ctes/cte_alias_lists/semantics.rs` | registered | no | 1 | required |
| `derived_fulltext_options::should_apply_fulltext_index_options_through_derived_sources` | [tests/sql_derived_sources.rs:236](../../../tests/sql_derived_sources.rs#L236) | `tests/sql_derived_sources/relations/derived_fulltext_options/semantics.rs` | registered | no | 1 | required |
