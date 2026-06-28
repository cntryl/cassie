# Feature Support

This matrix records Cassie's supported feature surface as an event-sourced read-model database. It covers implemented behavior as well as experimental and planned areas that need compatibility or production-readiness work.

Scope is determined by read-model usefulness, not database taxonomy. Relational, analytical, search, vector, and time-series capabilities belong here when they are needed to serve real projection workloads.

Status terms:

- `Stable`: implemented, tested, documented, and intended to remain compatible within the same major line.
- `Experimental`: implemented or partially implemented, but compatibility or output shape may still change.
- `Planned`: accepted roadmap area without a production guarantee.
- `Cassie-specific`: intentionally exposes Cassie behavior rather than PostgreSQL parity.

Experimental surfaces require the evidence gates in [Experimental Promotion Criteria](experimental-promotion-criteria.md) before they can be promoted or narrowed.

## SQL Query Features

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Query | SELECT, FROM, WHERE | Stable | PostgreSQL-like |
| Projection | `*`, explicit columns, aliases, expressions, scalar functions | Stable | PostgreSQL-like |
| Filtering | `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, AND, OR, NOT | Stable | PostgreSQL-like |
| Nulls | IS NULL, IS NOT NULL | Stable | PostgreSQL-like |
| Lists | IN, NOT IN | Stable | PostgreSQL-like |
| Ranges | BETWEEN, NOT BETWEEN | Stable | PostgreSQL-like |
| Ordering | ORDER BY, ASC, DESC, NULLS FIRST, NULLS LAST, aliases | Stable | PostgreSQL-like |
| Pagination | LIMIT, OFFSET | Stable | PostgreSQL-like |
| Deduplication | DISTINCT, DISTINCT ON | Stable | PostgreSQL-like |
| Aggregates | count, sum, avg, min, max | Stable | PostgreSQL-like |
| Grouping | GROUP BY, HAVING | Stable | PostgreSQL-like |
| Joins | INNER JOIN, LEFT JOIN, RIGHT JOIN, FULL OUTER JOIN, CROSS JOIN | Stable | PostgreSQL-like |
| Semi/anti joins | EXISTS, NOT EXISTS | Stable | PostgreSQL-like |
| Lateral | LATERAL, CROSS APPLY, OUTER APPLY | Stable | PostgreSQL-like with Cassie syntax support |
| Subqueries | scalar subqueries, FROM subqueries, predicate subqueries, correlated subqueries | Stable | PostgreSQL-like |
| CTEs | WITH, WITH RECURSIVE | Stable | PostgreSQL-like |
| Set operations | UNION, UNION ALL, INTERSECT, EXCEPT | Stable | PostgreSQL-like |
| Window functions | row_number, rank, dense_rank, lag, lead, first_value, last_value, supported frames | Stable | PostgreSQL-like with documented frame limits |
| DML | INSERT, UPDATE, DELETE, RETURNING, `COPY ... FROM STDIN WITH (FORMAT csv)` | Stable/Experimental | PostgreSQL-like plus simple-query CSV bulk load |
| DDL | CREATE TABLE, ALTER TABLE, DROP TABLE, CREATE SCHEMA, DROP SCHEMA, CREATE INDEX, DROP INDEX, CREATE ROLLUP, REFRESH ROLLUP, DROP ROLLUP, CREATE/ALTER/DROP/ENFORCE RETENTION POLICY | Stable/Experimental by object type | PostgreSQL-like plus Cassie-specific analytics |
| Transactions | BEGIN, COMMIT, ROLLBACK, savepoints | Stable | PostgreSQL-like with Cassie/Midge durability notes |
| Views | CREATE VIEW, DROP VIEW, nested views | Stable | PostgreSQL-like read-only view behavior |
| Functions | scalar functions, user-defined functions | Stable/Experimental | PostgreSQL-like where documented |
| Procedures | CREATE PROCEDURE, CALL | Experimental | Limited compatibility/admin surface, not a business-logic platform |
| Types | text, bool, integers, floats, decimal, timestamp, uuid, json, arrays, vector | Stable/Experimental | PostgreSQL-like plus Cassie vector |
| Casts | CAST(x AS type), x::type | Stable | PostgreSQL-like |

## Cassie-Specific Query Features

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Full-text | search(field, query), search_score(field, query), snippet(field, query) | Stable | Cassie-specific |
| Vector | vector_score, vector_distance, cosine_distance, dot_product, l2_distance | Stable | Cassie-specific |
| pgvector syntax | `<=>`, `<->`, `<#>`, vector(n) | Stable/Experimental | pgvector-style, not full extension parity |
| Hybrid | hybrid_score(text_score, vector_score) | Stable | Cassie-specific |
| Graph | CREATE GRAPH, graph_neighbors, graph_expand, graph_shortest_path | Experimental | Cassie-specific graph retrieval over read-model projections |
| Embeddings | provider, model, dimensions, metric validation | Experimental | Cassie-specific |
| Projections | projection metadata, source checkpoints, freshness, replay batch diagnostics, schema version, offset, lag, rebuild state | Experimental | Cassie-specific |
| Projection lifecycle | internal idempotent replay ingestion, materialized projections, analytical projection options, versioned builds, verification-aware active-version swaps, operations views | Experimental | Cassie-specific |
| Time series | time_bucket fixed windows, exact-match materialized rollups over deterministic aggregates, explicit retention policies, bucket-native range membership with row-backed fallback | Experimental | Cassie-specific deterministic semantics |
| Verification | deterministic row hashes, range hashes, projection roots, rebuild verification metadata, `VERIFY PROJECTION`, `DIFF PROJECTION`, `COMPARE PROJECTION`, `PLAN REPAIR PROJECTION`, `REPAIR PROJECTION`, local integrity reports, repair audit reports, admin multi-instance consistency manifests/reports | Experimental | Cassie-specific |

## Procedure Boundary

`CREATE PROCEDURE` and `CALL` are limited experimental compatibility/admin features.
They can wrap supported Cassie SQL statements for simple operational workflows, but they are not a stored-procedure business-logic platform.

Unsupported procedural expectations include:

- PL/pgSQL or other PostgreSQL procedural languages.
- Triggers or trigger-driven business logic.
- Transaction control inside procedure bodies.
- Recursive procedure calls.
- Dynamic SQL execution, cursors, exception blocks, temp-table orchestration, or server-side application frameworks.
- OLTP-style business logic that depends on locks, trigger ordering, PostgreSQL storage behavior, or full PostgreSQL catalog semantics.

## Index Support

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Primary | primary key index | Stable | PostgreSQL-like |
| Secondary | single-column index | Stable | PostgreSQL-like |
| Composite | multi-column index | Stable | PostgreSQL-like |
| Unique | unique index / unique constraint | Stable | PostgreSQL-like |
| Covering | INCLUDE (...) | Stable | PostgreSQL-like syntax with Cassie planner behavior |
| Partial | CREATE INDEX ... WHERE ... | Experimental | PostgreSQL-like syntax; limited predicate implication |
| Expression | CREATE INDEX ON table (lower(email)) | Experimental | PostgreSQL-like syntax; Cassie expression equivalence |
| Full-text | inverted index | Stable | Cassie-specific |
| Vector | brute force, HNSW, IVFFlat | Stable/Experimental | Cassie-specific with pgvector-style operators |
| Hybrid | text candidate plus vector rerank metadata | Stable | Cassie-specific |
| Graph | outbound/inbound adjacency sidecars for graph edge tables | Experimental | Cassie-specific |
| Column-store | USING column indexes, compressed column batches, covered scan acceleration, segment pruning | Stable | Cassie-specific |
| Time-series | timestamp range index metadata and planner selection | Experimental | Cassie-specific |
| Merkle | integrity index | Planned | Cassie-specific |

## Constraint Support

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Identity | PRIMARY KEY | Stable | PostgreSQL-like |
| Nullability | NOT NULL | Stable | PostgreSQL-like |
| Uniqueness | UNIQUE | Stable | PostgreSQL-like |
| Validation | CHECK | Stable | PostgreSQL-like |
| Defaults | DEFAULT | Stable | PostgreSQL-like |
| References | FOREIGN KEY | Stable/Experimental | PostgreSQL-like with documented limits |
| Generated | generated columns | Stable/Experimental | PostgreSQL-like with documented limits |

## Planner and Executor Support

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Frontend | lexer, parser, AST | Stable | Internal |
| Binding | name resolution, type resolution, function resolution, parameter binding | Stable | Internal |
| Plans | logical plan, physical plan | Stable | Internal |
| Optimization | predicate pushdown, projection pruning, limit pushdown, index selection | Stable | Semantics-preserving |
| Sorting | full sort, partial sort, top-k | Stable | Semantics-preserving |
| Joins | nested-loop, hash join, merge join, vectorized inner/left equi-join, semi join, anti join | Stable/Experimental | Semantics-preserving |
| Aggregation | hash aggregate, sort aggregate | Stable | Semantics-preserving |
| Distinct | hash distinct, sort distinct | Stable | Semantics-preserving |
| Execution | row executor, batch/vectorized executor | Stable/Experimental | Internal |
| Caching | plan cache, function cache, prepared statement cache | Stable/Experimental | Internal |
| Adaptive | runtime stats, cardinality feedback, adaptive candidate sizing, operator-selection feedback, prevalidated adaptive read-operator choices, runtime operator switching for prevalidated join fallback pairs | Experimental | Internal; advanced feedback paths are disabled by default unless explicitly enabled |
| Parallel | parallel scan, parallel scoring, parallel aggregation | Experimental | Semantics-preserving |

## Protocol and API Support

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| PostgreSQL wire | startup, auth, simple query, extended query, parse, bind, describe, execute, sync, close, simple-query COPY FROM STDIN CSV | Stable/Experimental | PostgreSQL-compatible subset |
| Pgwire results | row description, data row, command complete, error response, ready for query | Stable | PostgreSQL-compatible subset |
| Pgwire compatibility | prepared statements, portals, text/binary formats, catalog introspection | Stable/Experimental | PostgreSQL-compatible subset |
| HTTP | SQL query, search query, vector query, hybrid query, document APIs, admin manifest export and consistency-check APIs | Stable/Experimental | Cassie REST API |
| Recovery | v1 local snapshots with `cassie-snapshot-manifest.json`, copied Midge data directory, manifest compatibility validation, restore to empty local data directory | Experimental | Cassie-specific local recovery; no remote orchestration or replication |
| Observability | EXPLAIN, EXPLAIN ANALYZE, query stats, operator stats, cost-model diagnostics, index used, index feedback marker, operator feedback state/reason/cost/confidence diagnostics, adaptive decision/alternative/guard diagnostics, runtime operator switch candidate/pair/threshold/reason diagnostics, join strategy/key/sort/vectorized/fallback diagnostics, time-series index diagnostics, column-batch index used, storage-mode diagnostics, aggregate acceleration, rollup rewrite selected, mixed execution stages, analytical projection markers, rows scanned | Experimental | PostgreSQL-like entry points with Cassie output |
| Projection operations | active version, source checkpoint, lag, freshness, rebuild state, verification state, root state, last replay batch, last error, version state | Experimental | Cassie-specific |
| Operational scale | local node identity, projection ownership, tenant routing hints, partition assignment metadata, generation/state, `pg_catalog.pg_operational_assignments`, external route/drain/move contract | Experimental | Cassie-specific metadata for external orchestration; no distributed query behavior |
| Metrics | latency, throughput, errors, storage-family operation counts, cache occupancy, adaptive candidate, adaptive plan, and runtime operator switch counters, join execution/strategy/row/vectorized batch/spill-fallback counters, projection replay/build/swap/stale/hash/verification/integrity/consistency/mixed-fallback counters, retention enforcement/delete/skip counters, rollup refresh/rewrite/fallback counters, time-series scan/bucket/fallback counters, column-batch scan/fallback/byte/segment/column counters, aggregate acceleration counters | Experimental | Cassie-specific |
| Capacity management | advisory sizing guide, capacity signals, operator thresholds, and manual benchmark workflow | Experimental | Cassie-specific operational documentation; no automatic admission control or distributed movement |

## Projection Verification Surfaces

- Cassie-owned Midge keys use the lexkey v2 storage layout. Existing v1 Midge directories with slash-delimited row keys, `doc:` legacy keys, or `__cassie__` key families are intentionally incompatible and must be recreated or rebuilt before startup.
- `CASSIE_OPERATOR_FEEDBACK_ENABLED=1` enables experimental operator-selection feedback. When unset, the planner stays on the deterministic base path and EXPLAIN reports feedback as ignored or disabled.
- `CASSIE_ADAPTIVE_EXECUTION_ENABLED=1` enables experimental adaptive selection among prevalidated read-operator alternatives. `CASSIE_ADAPTIVE_MIN_COST_SAVINGS_BPS` controls the minimum observed savings required before an adaptive alternative replaces the base operator. `CASSIE_ADAPTIVE_MIN_CONFIDENCE_BPS` optionally requires a minimum operator-feedback confidence score before adaptive selection can pass; it defaults to `0`.
- `CASSIE_OPERATOR_SWITCHING_ENABLED=1` enables experimental runtime switching for explicitly prevalidated switch pairs. The first supported pair is `vectorized_join_to_merge_join`, which replays left/right join inputs before emitting rows when `CASSIE_OPERATOR_SWITCH_JOIN_ROW_THRESHOLD` is exceeded.
- `CREATE TABLE ... WITH (storage = column_store)` creates a column-store table. `pg_catalog.pg_table_storage` and EXPLAIN `storage_mode` expose the effective table mode (`row-store`, `column-indexed`, or `column-store`).
- `CASSIE_VECTORIZED_JOINS_ENABLED=1` enables the experimental vectorized inner/left equi-join executor path. `CASSIE_VECTORIZED_JOIN_BATCH_SIZE` bounds probe batch size, and EXPLAIN reports `vectorized_join_candidate`, `vectorized_join_enabled`, `vectorized_join_batch_size`, and `vectorized_join_fallback_reason`.
- Schema changes publish a new schema epoch for new queries while destructive table, index, view, and column cleanup is deferred until older active query epochs drain.

- `VERIFY PROJECTION <name> [VERSION <version_id>] [MODE metadata_only|hashes_only|indexes_only|full]` runs a read-only local integrity check and persists the latest report.
- `DIFF PROJECTION <left> [VERSION <version_id>] WITH <right> [VERSION <version_id>] [LIMIT n] [AFTER cursor]` returns deterministic local hash differences or an explicit unverifiable result.
- `COMPARE PROJECTION <name> [VERSION <version_id>] WITH MANIFEST '<json>'` compares the current local root digest with an imported manifest digest.
- `PLAN REPAIR PROJECTION <name> [VERSION <version_id>] SCOPE row|range|index|projection-version|full-rebuild` returns a deterministic admin dry-run plan from the latest local integrity report. The plan includes scope, affected-count metadata, the intended action, whether Cassie can execute it locally, and the required follow-up verification command.
- `REPAIR PROJECTION <name> [VERSION <version_id>] SCOPE row|range` executes the local hash-rebuild repair path only when the latest integrity report marks row/range findings repairable. Cassie immediately runs `VERIFY PROJECTION ... MODE full`, persists an audit report, and keeps unsupported scopes such as index, projection-version, and full-rebuild as deterministic errors until an explicit safe implementation exists. See [Projection Repair Runbook](projection-repair-runbook.md) for the operator workflow.
- `POST /v1/admin/projections/{projection}/verification-manifest` exports an authenticated versioned verification manifest with instance, schema, source checkpoint, hash metadata, root/range summaries, optional row-hash summaries, generated/expiration timestamps, and a canonical manifest digest. It excludes row values, vectors, text bodies, bind values, and credentials.
- `POST /v1/admin/projection-consistency-checks` imports two or more manifests and persists a deterministic offline report with `consistent`, `divergent`, `stale`, `incompatible`, or `unverifiable` state. This is an admin workflow only; query planning and execution do not wait on remote manifest checks.
- `pg_catalog.pg_projection_hashes` exposes row/range/root hash state, algorithm metadata, coverage counts, and root digest.
- `pg_catalog.pg_projection_operations` exposes freshness, rebuild, active-version, verification, and root state.
- `pg_catalog.pg_projection_integrity_reports` exposes the latest local integrity report.
- `pg_catalog.pg_projection_repair_reports` exposes persisted local repair audit records, including source integrity counts, scope, action, executable flag, verification requirement, and post-verification state.
- `pg_catalog.pg_projection_comparison_reports` exposes persisted local-vs-manifest comparison reports after restart hydration.
- `pg_catalog.pg_projection_consistency_reports` exposes persisted multi-instance consistency reports, including manifest count, instance ids, mismatch counts, stale/incompatible/unverifiable counts, and deterministic diagnostic samples after restart hydration.
- `pg_catalog.pg_operational_assignments` exposes local assignment metadata for external node, tenant, partition, and projection routing. Cassie stores and reports this metadata and documents route/drain/move semantics, but does not route, forward, fan out, or filter queries from it.
- `Cassie::create_snapshot_from_data_dir` and `Cassie::restore_snapshot` provide local snapshot/restore admin APIs around a Cassie manifest and copied Midge data directory. External tooling owns scheduling, transport, retention, encryption, and failover routing.
- Repair is admin-only and never automatic in query planning or execution. It is local to the Cassie instance and does not imply distributed replication, quorum, remote mutation, or cross-node reconciliation.
- EXPLAIN includes `cost_model`, `selected_cost`, `rejected_alternatives`, `operator_feedback`, `operator_feedback_reason`, `operator_feedback_base_candidate`, `operator_feedback_selected_candidate`, `operator_feedback_base_cost`, `operator_feedback_adjusted_cost`, `operator_feedback_confidence_bps`, `operator_feedback_age_ms`, `operator_feedback_samples`, `operator_feedback_outliers`, `adaptive_plan_enabled`, `adaptive_decision_point`, `adaptive_candidates`, `adaptive_base_alternative`, `adaptive_selected_alternative`, `adaptive_guard`, `adaptive_guard_passed`, `adaptive_reason`, `adaptive_diagnostic`, `operator_switch_candidate`, `operator_switch_enabled`, `operator_switch_pair`, `operator_switch_threshold`, `operator_switch_reason`, `join_strategy`, `join_keys`, `join_sort_required`, `join_fallback_reason`, `vectorized_join_candidate`, `vectorized_join_enabled`, `vectorized_join_batch_size`, `vectorized_join_fallback_reason`, `mixed_execution`, `mixed_stages`, `exact_baseline`, `analytical_projection`, `projection_freshness`, and graph `access_path=graph_adjacency` diagnostics for mixed search/vector/analytical/graph plans.

## Compatibility Notes

- See [PostgreSQL Compatibility](postgres-compatibility.md) for supported, unsupported, and intentionally different PostgreSQL behavior.
- See [Product Roadmap](product-roadmap.md) for milestone grouping and remaining roadmap themes.
- See [Production Readiness](production-readiness.md) for feature-family readiness, evidence, operational signals, restart coverage, and blockers.
- See [Capacity Management](capacity-management.md) for advisory capacity signals, thresholds, and operator actions.
- See [Definition of Done](definition-of-done.md) for how a feature moves from implemented to stable or production-ready.
