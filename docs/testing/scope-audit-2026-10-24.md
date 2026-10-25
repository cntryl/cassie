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
