# Inventory summary

Generated from `inventory.json`. Counts describe source declarations and static registrations, not test outcomes.

| Measure | Count |
| --- | ---: |
| Files under `tests/` | 1714 |
| Rust files | 1683 |
| Rust test declarations | 8192 |
| Python test declarations | 16 |
| Static Rust registration instances | 8320 |
| Cargo test targets | 32 |
| Reconciled backfill names | 138 |
| Unregistered Rust declarations | 0 |
| Unresolved module paths | 0 |
| Duplicate registered test identities | 0 |
| Repeated module registrations | 0 |
| Files with a reviewed physical placement | 1327 |
| Targets absent from exactly one CI shard | 0 |
| Existing files over their line limit | 0 |
| Destination name conflicts | 0 |

Shared support tests compile into several Cargo suites, so registration instances exceed physical source declarations. `cfg` expressions are not evaluated.

## Existing tests by owner

Each declaration appears on one owner page. Shared helper tests appear under `support`; their individual suite instances are retained in the JSON.

| Owner | Source declarations | Review required |
| --- | ---: | ---: |
| [analytics](inventory/analytics.md) | 176 | 0 |
| [backend_ci](inventory/backend_ci.md) | 2 | 0 |
| [bench_operations](inventory/bench_operations.md) | 16 | 0 |
| [catalog_security](inventory/catalog_security.md) | 404 | 0 |
| [compatibility](inventory/compatibility.md) | 705 | 0 |
| [configuration_isolated](inventory/configuration_isolated.md) | 17 | 0 |
| [constraint_ingress](inventory/constraint_ingress.md) | 3 | 0 |
| [domain_models](inventory/domain_models.md) | 87 | 0 |
| [executor](inventory/executor.md) | 108 | 0 |
| [expression_index_upsert](inventory/expression_index_upsert.md) | 4 | 0 |
| [json_writes](inventory/json_writes.md) | 20 | 0 |
| [metrics](inventory/metrics.md) | 58 | 0 |
| [null_safe_predicates](inventory/null_safe_predicates.md) | 42 | 0 |
| [parser_types](inventory/parser_types.md) | 367 | 0 |
| [pg18_parity](inventory/pg18_parity.md) | 16 | 0 |
| [pgwire_core](inventory/pgwire_core.md) | 81 | 0 |
| [pgwire_extended](inventory/pgwire_extended.md) | 3125 | 0 |
| [planner_controls](inventory/planner_controls.md) | 123 | 0 |
| [query_scan_controls](inventory/query_scan_controls.md) | 66 | 0 |
| [relational_qualification](inventory/relational_qualification.md) | 75 | 0 |
| [rest](inventory/rest.md) | 98 | 0 |
| [search](inventory/search.md) | 62 | 0 |
| [sql_contextual_literals](inventory/sql_contextual_literals.md) | 4 | 0 |
| [sql_derived_sources](inventory/sql_derived_sources.md) | 7 | 0 |
| [sql_exists_subqueries](inventory/sql_exists_subqueries.md) | 51 | 0 |
| [sql_mutations](inventory/sql_mutations.md) | 304 | 0 |
| [sql_queries](inventory/sql_queries.md) | 1831 | 0 |
| [sql_relation_identifiers](inventory/sql_relation_identifiers.md) | 8 | 0 |
| [sql_set_operations](inventory/sql_set_operations.md) | 8 | 0 |
| [storage_indexes](inventory/storage_indexes.md) | 161 | 0 |
| [support](inventory/support.md) | 9 | 0 |
| [temporal_predicate_literals](inventory/temporal_predicate_literals.md) | 1 | 0 |
| [time_series_float_partitions](inventory/time_series_float_partitions.md) | 1 | 0 |
| [vector_embeddings](inventory/vector_embeddings.md) | 168 | 0 |

## Cargo registrations

Suite names and CI shard assignments are preserved during file moves.

| Cargo target | Root | CI shard |
| --- | --- | --- |
| `analytics` | `tests/analytics.rs` | analytics-and-catalog |
| `backend_ci` | `tests/backend_ci.rs` | core |
| `bench_operations` | `tests/bench_operations.rs` | core |
| `catalog_security` | `tests/catalog_security.rs` | analytics-and-catalog |
| `compatibility` | `tests/compatibility.rs` | analytics-and-catalog |
| `configuration_isolated` | `tests/configuration_isolated.rs` | analytics-and-catalog |
| `constraint_ingress` | `tests/constraint_ingress.rs` | sql-semantics |
| `domain_models` | `tests/domain_models.rs` | analytics-and-catalog |
| `executor` | `tests/executor.rs` | core |
| `expression_index_upsert` | `tests/expression_index_upsert.rs` | sql-semantics |
| `json_writes` | `tests/json_writes.rs` | core |
| `metrics` | `tests/metrics.rs` | core |
| `null_safe_predicates` | `tests/null_safe_predicates.rs` | sql-semantics |
| `parser_types` | `tests/parser_types.rs` | core |
| `pgwire_core` | `tests/pgwire_core.rs` | core |
| `pgwire_extended` | `tests/pgwire_extended.rs` | pgwire-extended |
| `planner_controls` | `tests/planner_controls.rs` | core |
| `query_scan_controls` | `tests/query_scan_controls.rs` | query-surface |
| `relational_qualification` | `tests/relational_qualification.rs` | sql-semantics |
| `rest` | `tests/rest.rs` | query-surface |
| `search` | `tests/search.rs` | query-surface |
| `sql_contextual_literals` | `tests/sql_contextual_literals.rs` | query-surface |
| `sql_derived_sources` | `tests/sql_derived_sources.rs` | query-surface |
| `sql_exists_subqueries` | `tests/sql_exists_subqueries.rs` | query-surface |
| `sql_mutations` | `tests/sql_mutations.rs` | sql-semantics |
| `sql_queries` | `tests/sql_queries.rs` | sql-semantics |
| `sql_relation_identifiers` | `tests/sql_relation_identifiers.rs` | sql-semantics |
| `sql_set_operations` | `tests/sql_set_operations.rs` | sql-semantics |
| `storage_indexes` | `tests/storage_indexes.rs` | storage-and-vector |
| `temporal_predicate_literals` | `tests/temporal_predicate_literals.rs` | storage-and-vector |
| `time_series_float_partitions` | `tests/time_series_float_partitions.rs` | storage-and-vector |
| `vector_embeddings` | `tests/vector_embeddings.rs` | storage-and-vector |

## Files to split

Root Cargo suites have a 12,000 line limit. Other Rust files have a 1,000 line limit.

None.

## Proposed groups requiring size review

These groups already exceed 800 lines of test bodies, before imports and helpers. Split further if the complete file would exceed 1,000 lines.

| Proposed file | Test body lines | Tests |
| --- | ---: | ---: |
| `tests/executor/commands/semantics.rs` | 810 | 13 |
| `tests/parser_types/grammar/indexes/semantics.rs` | 878 | 27 |
| `tests/pgwire_extended/grammar/values_casts_uuid.rs` | 849 | 24 |
| `tests/pgwire_extended/type_resolution/output_source_seams.rs` | 847 | 14 |
| `tests/pgwire_extended/types/bit_bytea/casts_int8.rs` | 812 | 45 |
| `tests/sql_mutations/insert/insert_values.rs` | 836 | 20 |
| `tests/sql_queries/functions/temporal/temporal_functions.rs` | 816 | 58 |
| `tests/sql_queries/joins/semantics.rs` | 840 | 13 |
| `tests/sql_queries/text_search/text_search.rs` | 869 | 39 |

## Backfill authoring status

Each item links to a registered assertion in the backfill list. Authoring status and Cassie execution status are separate.

| Status | Names |
| --- | ---: |
| `authored` | 124 |
| `existing_equivalent` | 13 |
| `oracle_corrected` | 1 |
