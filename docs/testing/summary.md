# Inventory summary

Generated from `inventory.json`. Counts describe source declarations and static registrations, not test outcomes.

| Measure | Count |
| --- | ---: |
| Files under `tests/` | 673 |
| Rust files | 642 |
| Rust test declarations | 5263 |
| Python test declarations | 16 |
| Static Rust registration instances | 5387 |
| Cargo test targets | 32 |
| Proposed backfill names | 138 |
| Unregistered Rust declarations | 0 |
| Unresolved module paths | 0 |
| Targets absent from exactly one CI shard | 0 |
| Existing files over their line limit | 18 |
| Proposed files with duplicate bare test names | 0 |

Shared support tests compile into several Cargo suites, so registration instances exceed physical source declarations. `cfg` expressions are not evaluated.

## Existing tests by owner

Each declaration appears on one owner page. Shared helper tests appear under `support`; their individual suite instances are retained in the JSON.

| Owner | Source declarations | Review required |
| --- | ---: | ---: |
| [analytics](inventory/analytics.md) | 176 | 162 |
| [backend_ci](inventory/backend_ci.md) | 2 | 2 |
| [bench_operations](inventory/bench_operations.md) | 16 | 0 |
| [catalog_security](inventory/catalog_security.md) | 309 | 228 |
| [compatibility](inventory/compatibility.md) | 660 | 261 |
| [configuration_isolated](inventory/configuration_isolated.md) | 17 | 17 |
| [constraint_ingress](inventory/constraint_ingress.md) | 3 | 3 |
| [domain_models](inventory/domain_models.md) | 87 | 77 |
| [executor](inventory/executor.md) | 108 | 104 |
| [expression_index_upsert](inventory/expression_index_upsert.md) | 4 | 4 |
| [json_writes](inventory/json_writes.md) | 20 | 20 |
| [metrics](inventory/metrics.md) | 58 | 58 |
| [null_safe_predicates](inventory/null_safe_predicates.md) | 42 | 0 |
| [parser_types](inventory/parser_types.md) | 338 | 263 |
| [pg18_parity](inventory/pg18_parity.md) | 16 | 0 |
| [pgwire_core](inventory/pgwire_core.md) | 76 | 76 |
| [pgwire_extended](inventory/pgwire_extended.md) | 1075 | 424 |
| [planner_controls](inventory/planner_controls.md) | 123 | 122 |
| [query_scan_controls](inventory/query_scan_controls.md) | 66 | 64 |
| [relational_qualification](inventory/relational_qualification.md) | 75 | 0 |
| [rest](inventory/rest.md) | 98 | 96 |
| [search](inventory/search.md) | 62 | 62 |
| [sql_contextual_literals](inventory/sql_contextual_literals.md) | 4 | 4 |
| [sql_derived_sources](inventory/sql_derived_sources.md) | 7 | 7 |
| [sql_exists_subqueries](inventory/sql_exists_subqueries.md) | 51 | 6 |
| [sql_mutations](inventory/sql_mutations.md) | 301 | 231 |
| [sql_queries](inventory/sql_queries.md) | 1142 | 375 |
| [sql_relation_identifiers](inventory/sql_relation_identifiers.md) | 8 | 8 |
| [sql_set_operations](inventory/sql_set_operations.md) | 8 | 8 |
| [storage_indexes](inventory/storage_indexes.md) | 148 | 138 |
| [support](inventory/support.md) | 9 | 0 |
| [temporal_predicate_literals](inventory/temporal_predicate_literals.md) | 1 | 1 |
| [time_series_float_partitions](inventory/time_series_float_partitions.md) | 1 | 1 |
| [vector_embeddings](inventory/vector_embeddings.md) | 168 | 153 |

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

- `tests/catalog_security/postgresql18_catalog_information.rs`
- `tests/catalog_security/type_catalog_discovery.rs`
- `tests/compatibility/lifecycle/ddl_extensions/table_forms.rs`
- `tests/compatibility/lifecycle/ddl_extensions.rs`
- `tests/compatibility/lifecycle/mutation_ddl.rs`
- `tests/compatibility/lifecycle/routine_objects.rs`
- `tests/compatibility/lifecycle/session_transaction.rs`
- `tests/parser_types/postgresql18_grammar.rs`
- `tests/pgwire_extended/parameter_occurrences.rs`
- `tests/pgwire_extended/postgresql18_numeric_compat.rs`
- `tests/pgwire_extended/postgresql18_string_compat.rs`
- `tests/pgwire_extended/postgresql18_temporal_compat.rs`
- `tests/pgwire_extended/scalar_subqueries/pg18_residuals.rs`
- `tests/pgwire_extended/transaction_boundaries.rs`
- `tests/pgwire_extended/type_array_text_inputs.rs`
- `tests/pgwire_extended/type_json_scalar_documents.rs`
- `tests/sql_queries/postgresql18_aggregate_query_gaps.rs`
- `tests/sql_queries/postgresql18_pattern_matching.rs`

## Proposed groups requiring size review

These groups already exceed 800 lines of test bodies, before imports and helpers. Split further if the complete file would exceed 1,000 lines.

| Proposed file | Test body lines | Tests |
| --- | ---: | ---: |
| `tests/compatibility/lifecycle/tables/table_forms/semantics.rs` | 828 | 11 |
| `tests/pgwire_extended/type_resolution/output_source_seams.rs` | 890 | 14 |
| `tests/sql_queries/functions/temporal/temporal_functions.rs` | 816 | 58 |
| `tests/sql_queries/text_search/text_search.rs` | 869 | 39 |

## Proposed backfill status

These are planning labels carried from the backfill list. They are not evidence of a failing or passing Cassie run.

| Status | Names |
| --- | ---: |
| `blocked` | 22 |
| `blocked_strengthen` | 1 |
| `planned` | 93 |
| `policy_blocked` | 14 |
| `strengthen` | 8 |
