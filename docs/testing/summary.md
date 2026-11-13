# Inventory summary

Generated from `inventory.json`. Counts describe source declarations and static registrations, not test outcomes.

| Measure | Count |
| --- | ---: |
| Files under `tests/` | 1645 |
| Rust files | 1608 |
| Rust test declarations | 9547 |
| Python test declarations | 22 |
| Static Rust registration instances | 9548 |
| Cargo test targets | 38 |
| Reconciled backfill names | 117 |
| Unregistered Rust declarations | 5 |
| Unresolved module paths | 0 |
| Duplicate registered test identities | 0 |
| Repeated module registrations | 0 |
| Files with a reviewed physical placement | 1264 |
| Existing files over their line limit | 1 |
| Destination name conflicts | 0 |

Registration instances count each executable owner of a source declaration. Shared fixture self-tests have one library owner; `cfg` expressions are not evaluated.

## Existing tests by owner

Each declaration appears on one owner page. Shared helper tests appear under `support`; their individual suite instances are retained in the JSON.

| Owner | Source declarations | Review required |
| --- | ---: | ---: |
| [analytics](inventory/analytics.md) | 185 | 0 |
| [bench_diagnostics](inventory/bench_diagnostics.md) | 2 | 0 |
| [bench_operations](inventory/bench_operations.md) | 16 | 0 |
| [catalog_security](inventory/catalog_security.md) | 476 | 0 |
| [compatibility](inventory/compatibility.md) | 476 | 0 |
| [composite_types](inventory/composite_types.md) | 149 | 0 |
| [configuration_isolated](inventory/configuration_isolated.md) | 17 | 0 |
| [constraint_ingress](inventory/constraint_ingress.md) | 3 | 0 |
| [domain_models](inventory/domain_models.md) | 87 | 0 |
| [domain_types](inventory/domain_types.md) | 135 | 0 |
| [executor](inventory/executor.md) | 108 | 0 |
| [expression_index_upsert](inventory/expression_index_upsert.md) | 4 | 0 |
| [json_writes](inventory/json_writes.md) | 21 | 0 |
| [metrics](inventory/metrics.md) | 58 | 0 |
| [null_safe_predicates](inventory/null_safe_predicates.md) | 65 | 0 |
| [parser_types](inventory/parser_types.md) | 406 | 0 |
| [pg18_parity](inventory/pg18_parity.md) | 16 | 0 |
| [pgwire_catalogs](inventory/pgwire_catalogs.md) | 605 | 0 |
| [pgwire_core](inventory/pgwire_core.md) | 84 | 0 |
| [pgwire_extended](inventory/pgwire_extended.md) | 975 | 0 |
| [pgwire_grammar](inventory/pgwire_grammar.md) | 1233 | 8 |
| [pgwire_types](inventory/pgwire_types.md) | 1150 | 0 |
| [planner_controls](inventory/planner_controls.md) | 124 | 0 |
| [query_scan_controls](inventory/query_scan_controls.md) | 66 | 0 |
| [relational_qualification](inventory/relational_qualification.md) | 76 | 0 |
| [rest](inventory/rest.md) | 100 | 0 |
| [search](inventory/search.md) | 64 | 0 |
| [sql_contextual_literals](inventory/sql_contextual_literals.md) | 4 | 0 |
| [sql_derived_sources](inventory/sql_derived_sources.md) | 7 | 0 |
| [sql_exists_subqueries](inventory/sql_exists_subqueries.md) | 54 | 0 |
| [sql_mutations](inventory/sql_mutations.md) | 464 | 0 |
| [sql_queries](inventory/sql_queries.md) | 1947 | 0 |
| [sql_relation_identifiers](inventory/sql_relation_identifiers.md) | 8 | 0 |
| [sql_set_operations](inventory/sql_set_operations.md) | 8 | 0 |
| [storage_indexes](inventory/storage_indexes.md) | 187 | 0 |
| [support](inventory/support.md) | 9 | 0 |
| [temporal_predicate_literals](inventory/temporal_predicate_literals.md) | 1 | 0 |
| [test_inventory](inventory/test_inventory.md) | 6 | 0 |
| [time_series_float_partitions](inventory/time_series_float_partitions.md) | 1 | 0 |
| [vector_embeddings](inventory/vector_embeddings.md) | 172 | 0 |

## Cargo targets

Each suite's Cargo target and source root are recorded here.

| Cargo target | Root |
| --- | --- |
| `analytics` | `tests/analytics.rs` |
| `bench_diagnostics` | `tests/bench_diagnostics.rs` |
| `bench_operations` | `tests/bench_operations.rs` |
| `cassie_test_support` | `tests/support/pgwire_helpers/lib.rs` |
| `catalog_security` | `tests/catalog_security.rs` |
| `compatibility` | `tests/compatibility.rs` |
| `composite_types` | `tests/composite_types.rs` |
| `configuration_isolated` | `tests/configuration_isolated.rs` |
| `constraint_ingress` | `tests/constraint_ingress.rs` |
| `domain_models` | `tests/domain_models.rs` |
| `domain_types` | `tests/domain_types.rs` |
| `executor` | `tests/executor.rs` |
| `expression_index_upsert` | `tests/expression_index_upsert.rs` |
| `json_writes` | `tests/json_writes.rs` |
| `metrics` | `tests/metrics.rs` |
| `null_safe_predicates` | `tests/null_safe_predicates.rs` |
| `parser_types` | `tests/parser_types.rs` |
| `pgwire_catalogs` | `tests/pgwire_catalogs.rs` |
| `pgwire_core` | `tests/pgwire_core.rs` |
| `pgwire_extended` | `tests/pgwire_extended.rs` |
| `pgwire_grammar` | `tests/pgwire_grammar.rs` |
| `pgwire_types` | `tests/pgwire_types.rs` |
| `planner_controls` | `tests/planner_controls.rs` |
| `query_scan_controls` | `tests/query_scan_controls.rs` |
| `relational_qualification` | `tests/relational_qualification.rs` |
| `rest` | `tests/rest.rs` |
| `search` | `tests/search.rs` |
| `sql_contextual_literals` | `tests/sql_contextual_literals.rs` |
| `sql_derived_sources` | `tests/sql_derived_sources.rs` |
| `sql_exists_subqueries` | `tests/sql_exists_subqueries.rs` |
| `sql_mutations` | `tests/sql_mutations.rs` |
| `sql_queries` | `tests/sql_queries.rs` |
| `sql_relation_identifiers` | `tests/sql_relation_identifiers.rs` |
| `sql_set_operations` | `tests/sql_set_operations.rs` |
| `storage_indexes` | `tests/storage_indexes.rs` |
| `temporal_predicate_literals` | `tests/temporal_predicate_literals.rs` |
| `time_series_float_partitions` | `tests/time_series_float_partitions.rs` |
| `vector_embeddings` | `tests/vector_embeddings.rs` |

## Files to split

Root Cargo suites have a 12,000 line limit. Other Rust files have a 1,000 line limit.

- `tests/parser_types/grammar/indexes/semantics.rs`

## Proposed groups requiring size review

These groups already exceed 800 lines of test bodies, before imports and helpers. Split further if the complete file would exceed 1,000 lines.

| Proposed file | Test body lines | Tests |
| --- | ---: | ---: |
| `tests/catalog_security/catalogs/types/catalog_discovery/semantics.rs` | 824 | 28 |
| `tests/compatibility/lifecycle/indexes/pgvector_profile.rs` | 825 | 10 |
| `tests/compatibility/lifecycle/tables/table_forms/semantics.rs` | 832 | 16 |
| `tests/parser_types/grammar/expressions/semantics.rs` | 816 | 37 |
| `tests/parser_types/grammar/indexes/semantics.rs` | 964 | 34 |
| `tests/pgwire_extended/functions/math/math.rs` | 908 | 16 |
| `tests/pgwire_extended/functions/math/overload_errors.rs` | 857 | 17 |
| `tests/pgwire_extended/grammar/bytea_functions_parameters.rs` | 895 | 11 |
| `tests/pgwire_extended/grammar/scalar_batch_09.rs` | 821 | 45 |
| `tests/pgwire_extended/grammar/values_casts_uuid.rs` | 849 | 24 |
| `tests/pgwire_extended/type_resolution/output_source_seams.rs` | 852 | 14 |
| `tests/pgwire_extended/types/bit_bytea/casts_int8.rs` | 812 | 45 |
| `tests/pgwire_extended/types/json/operator_overload/semantics.rs` | 810 | 30 |
| `tests/pgwire_extended/types/temporal/binary.rs` | 897 | 13 |
| `tests/sql_mutations/insert/insert_values.rs` | 839 | 20 |
| `tests/sql_queries/aggregates/windows/aggregate_query/semantics.rs` | 875 | 24 |
| `tests/sql_queries/functions/temporal/temporal_functions.rs` | 816 | 58 |
| `tests/sql_queries/joins/semantics.rs` | 827 | 13 |

## Backfill authoring status

Each item links to a registered assertion in the backfill list. Authoring status and Cassie execution status are separate.

| Status | Names |
| --- | ---: |
| `authored` | 103 |
| `existing_equivalent` | 13 |
| `oracle_corrected` | 1 |
