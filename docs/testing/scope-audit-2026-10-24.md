# Test scope reconciliation: native MONEY

The [Product Roadmap](../product-roadmap.md) selects a read-model database, not
PostgreSQL server parity. It explicitly requires removal of out-of-scope tests
rather than implementing their features or marking them ignored. Native MONEY
is not selected by the [Type Contract](../type-contract.md). Live
[#908](https://github.com/cntryl/cassie/issues/908), read on 2026-10-24, states
that this family lacks a selected wire/durable contract and is outside active
release milestones. Exact NUMERIC remains selected for decimal read-model data.

The user delegated decisions to Jev and reaffirmed Cassie's product goals.
Jev reviewed each dedicated case against the current roadmap, type contract
and live issue and selected removal for all 30 cases. Source inspection confirms
that every case requires native MONEY values or OID 790 before it can prove
its expectation. Even malformed-input and AVG-error cases presuppose the absent
type, rather than testing rejection of an unselected family. No selected
NUMERIC, transaction, pgwire recovery or supported-type test is removed.

These are scope removals, not runtime fixes or passing conformance cells.
#908 stays open and unimplemented. Native MONEY remains unsupported. Historical
sources remain recoverable from base commit `3ec018b60f6861cd4877edd0d8446a727a7a39b8`.
Local source hashes, original bodies, Jev request/response and decision evidence
are retained under ignored `target/batched-runtime/batch-16/`.

## Removed cases

| Original source | Case | Scope reason |
| --- | --- | --- |
| `tests/pgwire_extended/types/numeric/money/semantics.rs` | `should_preserve_money_numeric_operator_types` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money_integer_division_rounding.rs` | `should_round_money_integer_division_at_signed_half_cent` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_return_c_locale_money_text_over_postgresql_wire` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_describe_typed_money_parameter_result` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_reformat_prepared_money_after_monetary_locale_change` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_report_money_overflow_with_numeric_value_out_of_range_sqlstate` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_round_money_casts_at_signed_half_cent_boundaries` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_check_money_signed_int64_lower_boundary` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_send_money_values_in_postgresql_binary_format` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_decode_binary_money_parameters_before_numeric_cast` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_decode_text_money_parameters_before_numeric_cast` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_report_malformed_money_text_with_invalid_text_representation_sqlstate` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_format_non_c_money_from_locale_specific_input` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_describe_money_mixed_arithmetic_with_postgresql_result_oids` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_report_money_addition_overflow_sqlstate` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_report_money_division_by_zero_sqlstate` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_recover_after_money_arithmetic_errors` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_send_negative_money_in_signed_postgresql_binary_format` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/money.rs` | `should_decode_negative_binary_money_parameters_as_negative_c_locale_text` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/semantics.rs` | `should_format_money_after_setting_c_monetary_locale` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/semantics.rs` | `should_report_money_average_as_undefined_function` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/types/money/restart.rs` | `should_preserve_money_binary_identity_given_inserted_and_updated_rows_when_storage_restarts` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/aggregates/money/money.rs` | `should_preserve_money_type_oids_through_sum` | Requires unselected native MONEY contract, #908. |
| `tests/pgwire_extended/aggregates/money/errors.rs` | `should_report_money_sum_overflow_with_numeric_value_out_of_range_sqlstate` | Requires unselected native MONEY contract, #908. |
| `tests/sql_queries/types/money/money.rs` | `should_preserve_money_arithmetic_precision` | Requires unselected native MONEY contract, #908. |
| `tests/sql_queries/types/money/money.rs` | `should_parse_money_text_compare_values_propagate_nulls` | Requires unselected native MONEY contract, #908. |
| `tests/sql_queries/types/money/money.rs` | `should_keep_money_text_values_across_restart` | Requires unselected native MONEY contract, #908. |
| `tests/sql_queries/aggregates/money/money.rs` | `should_sum_multiple_money_values_exactly_skipping_nulls` | Requires unselected native MONEY contract, #908. |
| `tests/sql_queries/aggregates/money/money.rs` | `should_keep_empty_input_money_aggregates_null_except_count` | Requires unselected native MONEY contract, #908. |
| `tests/sql_queries/aggregates/money/money.rs` | `should_keep_all_null_money_aggregates_null` | Requires unselected native MONEY contract, #908. |

## PostgreSQL type-catalog scope reconciliation (2026-10-25)

Jev reviewed the active Type Contract, Product Roadmap, and live issues #905
and #908. It selected removal of two catalog tests whose expected rows require
native MONEY, XML, geometric, or range/multirange families. Those families are
explicitly unselected; no runtime catalog rows or type support were added.

| Original source | Removed case | Scope reason |
| --- | --- | --- |
| `tests/catalog_security/catalogs/types/catalog_discovery/metadata.rs` | `should_discover_postgresql_catalog_rows_for_supported_type_families` | Every expected row is an unselected native MONEY, XML, geometric, range, or multirange identity. |
| `tests/catalog_security/catalogs/types/catalog_information/semantics.rs` | `should_resolve_builtin_range_subtype_in_pg_range` | Requires the unselected range/multirange type family owned by #905. |

Selected array TypeInfo/COPY coverage and other catalog behavior remain in the
suite. These removals are scope reconciliation; they do not count as passing
conformance for the removed capabilities.

## Vector catalog count assertion (2026-10-25)

Jev reviewed the failure in `should_deduplicate_supported_schema_vector_identities`.
Its exact count of 32 covered the entire `pg_type` registry even though the
test owns vector-dimension deduplication; the runtime correctly exposes 86
existing scalar and array identities from the finite built-in registry. The
stale count assertion was removed. The test still requires the exact three
schema-referenced vector identities, checks all OIDs are unique, and protects
the PostgreSQL BOOLEAN[] identity from replacement by an excluded
ARRAY(VECTOR) identity. The #843/#844 v0.4 owners remain open.

## PostgreSQL catalog contract reconciliation (2026-10-25)

Jev reviewed the 95-failure inventory against the v0.3 roadmap, #817 and the
selected catalog read-model checkpoint in `docs/catalog-support.md`. It chose
to preserve the supported native NAME descriptor path and park expectations
that require persistent relation ownership, native information-schema domain
OIDs/codecs, PostgreSQL physical relation storage, full session/lock catalogs,
pretty-printed constraint definitions, or empty stand-ins for system catalog
families that Cassie does not implement. Those tests were removed from the
active green suite rather than turning absent facts into fabricated rows.

The following cases remain pending under the listed contract owners; their
removal is scope reconciliation, not a passing result for the PostgreSQL
capability:

| Original source | Removed cases | Scope reason |
| --- | --- | --- |
| `tests/catalog_security/catalogs/columns/catalog_relation_shapes.rs` | `should_describe_pg_opfamily_columns`, `should_describe_pg_rewrite_columns`, `should_describe_pg_statistic_columns`, `should_describe_pg_text_search_dictionary_columns`, `should_describe_pg_text_search_parser_columns`, `should_describe_pg_text_search_template_columns` | Current checkpoint omits these relation shapes until Cassie has real semantic rows. #817 remains open. |
| `tests/catalog_security/catalogs/constraints/information_schema.rs` | `should_render_created_check_constraint_definition` | PostgreSQL pretty-printed constraint definitions remain unqualified under #817. |
| `tests/catalog_security/catalogs/identity_privileges/information_schema.rs` | `should_report_owner_privileges_in_information_schema_role_table_grants`, `should_report_owner_privileges_in_information_schema_table_privileges` | Relation owners and object privilege ACLs are not stored; ownership/privilege implementation remains under #831. |
| `tests/catalog_security/catalogs/types/information_schema.rs`, `tests/catalog_security/catalogs/routines/information_schema.rs`, `tests/catalog_security/catalogs/triggers/information_schema.rs` | `should_report_array_element_metadata_in_information_schema`, `should_report_builtin_parameter_metadata_in_information_schema`, `should_describe_trigger_metadata_without_trigger_objects` | These expectations require native information-schema domain identities/codecs or unsupported trigger metadata; those identities and trigger capability are not selected. See #817 and #837. |
| `tests/catalog_security/catalogs/relations/size_information.rs` | `should_keep_created_catalog_rows_visible_without_schema_usage` | Requires REVOKE/ACL behavior not yet implemented under #831. |
| `tests/catalog_security/catalogs/sizes/size_information.rs`, `tests/catalog_security/catalogs/sequences/size_information.rs` | `should_report_relation_fork_sizes_for_a_created_table`, `should_resolve_relation_size_names_through_search_path`, `should_report_relation_size_components`, `should_report_toast_table_sizes`, `should_resolve_database_size_overloads`, `should_report_sizes_for_a_sequence_relation` | Cassie does not expose PostgreSQL physical relation/database fork sizes or TOAST storage. These do not measure Midge storage. |
| `tests/catalog_security/catalogs/relations/system_session.rs`, `tests/catalog_security/catalogs/system_information/system_session.rs` | `should_observe_the_current_relation_lock_in_pg_locks`, `should_observe_the_current_backend_in_pg_stat_activity` | Complete PostgreSQL lock and backend activity catalogs remain unqualified; `LOCK TABLE` and server lock-manager semantics are not selected. |
| `tests/catalog_security/catalogs/routines/object_identity_functions.rs` | `should_identify_builtin_routine_with_catalog_qualified_signature` | Requires PostgreSQL `regprocedure`/OID object-identity lifecycle, which remains unqualified under #817. |

The size-format tests were narrowed to the already selected `pg_size_pretty(bigint)`
contract. The numeric overload remains unsupported as recorded in
`docs/catalog-support.md`; its test now covers selected BIGINT and NULL input.
The `pg_tables`/`pg_views` row tests continue to cover actual object discovery,
native NAME descriptors, index flags and stored view text while asserting NULL
for unrecorded owners. `should_report_positive_size_for_the_active_database` was
also removed from `tests/catalog_security/catalogs/sizes/catalog_lookup.rs`;
`pg_database_size` is a PostgreSQL physical-storage helper and Cassie does not
claim Midge size as that value.

Jev reviewed the additive `FunctionId` entries needed to register the selected
built-in signature and no-temporary-schema helpers. It recommended proceeding
(0.98 probability, confidence 0.97): the SQL behavior is explicitly requested
by the current #817 tests, the Rust surface is pre-1.0 and not documented as a
stable compatibility contract, and the change adds no persistent representation.

## Core catalog metadata and experimental test reconciliation (2026-10-25)

After commit `f0dc5481`, the full suite had 64 failures. Jev selected the finite
#817 core catalog slice (probability 1.0): real current-object
`pg_class`/`pg_namespace`/`pg_attribute`/`pg_constraint` identities and pinned
built-in `pg_proc` rows. It rejected prioritizing #818 object identity, #831
ownership, or broad server/system catalog parity. The three current-object
tests now compare the documented logical text carriers (`relnamespace`,
`conrelid`, `attrelid`) to text names rather than comparing them with OID
values. A separate finite `pg_proc` relation exposes only the pinned built-in
name, OID and result-type fields used by #817; it does not invent user-routine
metadata.

Jev reviewed the conflict where the new #817 catalog test expected PostgreSQL
generated NOT NULL names (`*_not_null`) and two already-passing workflow tests
expected Cassie's earlier `*_n` spelling. It chose PostgreSQL catalog names
(probability 0.63, confidence 0.45). The runtime now keeps the native spelling
in `pg_constraint`; the pgAdmin and foreign-key workflow expectations were
updated, while non-catalog constraint helpers retain their existing names.

The following failing tests were removed from the active suite because their
capability is outside the selected v0.3 catalog contract or has an open
prerequisite. This records a scope disposition, not passing evidence for the
removed PostgreSQL behavior:

| Source | Removed tests | Disposition |
| --- | --- | --- |
| `catalogs/backend_statistics.rs`, `catalogs/blocking.rs`, `catalogs/commit_control.rs`, `catalogs/multixact.rs`, `catalogs/snapshots.rs`, `catalogs/statistics_restore.rs`, `catalogs/transactions.rs` | `should_scope_backend_statistics_given_multiple_sessions_when_io_and_wal_are_queried`; `should_cancel_the_target_query_given_an_authorized_session_when_pg_cancel_backend_is_called`; `should_terminate_the_target_session_given_an_authorized_session_when_pg_terminate_backend_is_called`; `should_report_lock_blockers_given_a_waiting_session_when_pg_blocking_pids_is_called`; `should_report_snapshot_blockers_given_a_deferrable_snapshot_waiter_when_safe_snapshot_blockers_are_queried`; both `should_follow_*_policy_*` commit/control tests; `should_report_multixact_members_given_shared_row_locks_when_members_are_queried`; all three snapshot/txid tests; all four statistics restore/clear tests; `should_report_transaction_status_given_assigned_transaction_ids_when_transaction_outcomes_are_queried` | Full PostgreSQL backend/lock manager, MVCC snapshot, commit timestamp, multixact and planner-statistics state is not part of the single-node read-model contract. |
| `catalogs/foreign_keys.rs`, `catalogs/object_addresses.rs`, `catalogs/system_information/catalog_lookup.rs`, `catalogs/relations/catalog_lookup.rs`, `catalogs/routines/catalog_lookup.rs`, `catalogs/columns/catalog_lookup.rs`, `catalogs/sequences/catalog_lookup.rs`, `catalogs/routines/populated_descriptions.rs`, `catalogs/descriptions/catalog_information/semantics.rs`, `catalogs/visibility/visibility_functions.rs` | `should_report_catalog_foreign_keys_given_builtin_catalogs_when_pg_get_catalog_foreign_keys_is_called`; both object-address tests; reserved-keyword catalog helper; positive txid helper; function/column/sequence privilege helpers; populated descriptions; all three unsupported built-in visibility/search-path tests | These functions require unselected PostgreSQL object-address, ACL/ownership, visibility or transaction identity surfaces; privilege metadata is owned by #831 and remaining object identity work by #818. |
| `catalogs/indexes/index_property_functions.rs`, `catalogs/indexes/catalog_information/semantics.rs`, `catalogs/relations/catalog_information/semantics.rs`, `catalogs/relations/catalog_information/metadata.rs`, `catalogs/columns/catalog_information/semantics.rs`, `catalogs/system_information/catalog_information/semantics.rs` | Three index property matrices; pg_stat user table/index rows; `pg_amop`/`pg_amproc`; built-in aggregate, conversion, operator class, language, text-search config, collation, tablespace and operator listings | These experimental built-in catalogs are outside #817's finite relation list; stats rows and operator-family/text-search stand-ins are explicitly omitted by the catalog checkpoint. |
| `catalogs/types/base_information_function.rs`, `catalogs/types/catalog/semantics.rs`, `catalogs/types/catalog/metadata.rs`, `catalogs/columns/catalog/semantics.rs`, `catalogs/types/catalog_discovery/metadata.rs`, `catalogs/types/text_search_identifier_lookup/semantics.rs`, `catalogs/types/text_search_identifier_lookup/restart.rs`, `catalogs/types/vector_catalog.rs` | `pg_basetype`; custom enum/domain/composite and system tuple-column tests; pgcrypto extension listing; user-created text-search configuration lookup/restart; `int2vector`/`oidvector` catalog identity tests | These require type families, system columns, extension lifecycle or user-created text-search objects not selected by the current Type Contract; extension compatibility remains with #843 and broader type/object identity work with #817/#818. |
