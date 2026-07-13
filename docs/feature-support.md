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
| Query | SELECT, FROM, WHERE | Stable/Experimental | PostgreSQL-like for table-backed reads and supported table-free literals, parameters, casts, and set operations |
| Projection | `*`, explicit columns, aliases, expressions, scalar functions | Stable/Experimental | PostgreSQL-like for covered expression/result-type shapes |
| Filtering | `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, AND, OR, NOT | Stable/Experimental | PostgreSQL-like three-valued semantics for compatible operands; binder rejects incompatible families |
| Nulls | IS NULL, IS NOT NULL, NULL in expressions and predicates | Stable/Experimental | Explicit null checks and unknown propagation are covered across expressions and filters |
| Lists | IN, NOT IN | Stable/Experimental | PostgreSQL-like, including NULL-containing list unknown results |
| Ranges | BETWEEN, NOT BETWEEN | Stable/Experimental | PostgreSQL-like, including NULL operand and bound propagation |
| Ordering | ORDER BY, ASC, DESC, NULLS FIRST, NULLS LAST, aliases | Stable | PostgreSQL-like |
| Pagination | LIMIT, OFFSET | Stable | PostgreSQL-like |
| Deduplication | DISTINCT, DISTINCT ON | Stable | PostgreSQL-like |
| Aggregates | count, sum, avg, min, max | Stable | PostgreSQL-like |
| Grouping | GROUP BY, HAVING | Stable | PostgreSQL-like |
| Joins | INNER JOIN, LEFT JOIN, RIGHT JOIN, FULL OUTER JOIN, CROSS JOIN | Stable/Experimental | PostgreSQL-like for covered predicates; NULL equality keys never match across nested-loop, merge, vectorized, bounded, and indexed paths |
| Semi/anti joins | EXISTS, NOT EXISTS | Stable | PostgreSQL-like |
| Lateral | LATERAL, CROSS APPLY, OUTER APPLY | Stable | PostgreSQL-like with Cassie syntax support |
| Subqueries | scalar subqueries, FROM subqueries, predicate subqueries, correlated subqueries | Stable | PostgreSQL-like |
| CTEs | WITH, WITH RECURSIVE | Stable/Experimental | Recursive terms use a generation-local working delta; UNION deduplicates, UNION ALL preserves duplicates, aliases and type/arity validation are deterministic, and depth/temp-memory limits remain enforced |
| Set operations | UNION, UNION ALL, INTERSECT, EXCEPT | Stable | PostgreSQL-like |
| Window functions | row_number, rank, dense_rank, lag, lead, first_value, last_value, documented ROWS frames | Stable/Experimental | Ordered defaults use a bounded ROWS contract; RANGE, GROUPS, and EXCLUDE return deterministic `0A000` unsupported errors |
| DML | INSERT, UPDATE, DELETE, RETURNING, `COPY ... FROM STDIN WITH (FORMAT csv)` | Stable/Experimental | PostgreSQL-like plus simple-query CSV bulk load |
| DDL | CREATE/DROP DATABASE, CREATE/ALTER/DROP TABLE, CREATE/ALTER/DROP SCHEMA, CREATE/DROP INDEX, CREATE ROLLUP, REFRESH ROLLUP, DROP ROLLUP, CREATE/ALTER/DROP/ENFORCE RETENTION POLICY | Stable/Experimental by object type | PostgreSQL-like plus Cassie-specific analytics |
| Session scope | `current_database()`, `current_schema()`, `SHOW search_path`, `SET search_path` | Stable | PostgreSQL-like current-database session model |
| Transactions | BEGIN, COMMIT, ROLLBACK, savepoints | Stable/Experimental | A transaction accepts only default/`READ COMMITTED` isolation, rejects DDL and COPY before mutation, stages one collection only, and preflights cross-collection foreign-key actions. Rejections use `0A000` and preserve prior staged state for rollback. Base writes become durable before the session is cleared; column-batch, projection-hash, rollup, and materialized-projection refreshes run afterward with generation-bound debt and startup replay, and a second COMMIT is rejected |
| Views | CREATE VIEW, DROP VIEW, nested views | Stable | PostgreSQL-like read-only view behavior |
| Functions | scalar functions, user-defined functions | Stable/Experimental | PostgreSQL-like where documented |
| Procedures | CREATE PROCEDURE, CALL | Experimental | Limited compatibility/admin surface, not a business-logic platform |
| Types | text, bool, integers, floats, decimal, timestamp, uuid, json, arrays, vector | Stable/Experimental | PostgreSQL-like plus Cassie vector |
| Casts | CAST(x AS type), x::type | Stable | PostgreSQL-like |

## Cassie-Specific Query Features

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Full-text | search(field, query), search_score(field, query), snippet(field, query) | Stable/Experimental | Cassie-specific |
| Vector | vector_score, vector_distance, cosine_distance, dot_product, l2_distance | Stable/Experimental | Cassie-specific |
| pgvector syntax | `<=>`, `<->`, `<#>`, vector(n) | Stable/Experimental | pgvector-style, not full extension parity |
| Hybrid | hybrid_score(text_score, vector_score) | Stable/Experimental | Cassie-specific |
| Graph | CREATE GRAPH, graph_neighbors, graph_expand, graph_shortest_path | Experimental | Cassie-specific graph retrieval over read-model projections |
| Embeddings | provider auth/config, model and dimension validation, timeout/retry/batch controls, hosted OpenAI-compatible providers, hosted Voyage/Cohere providers, and deterministic local hash embeddings | Experimental | Cassie-specific |
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
| Full-text | inverted index | Stable/Experimental | Cassie-specific |
| Vector | brute force, HNSW, IVFFlat | Stable/Experimental | Cassie-specific with pgvector-style operators |
| Hybrid | text candidate plus vector rerank metadata | Stable/Experimental | Cassie-specific |
| Graph | outbound/inbound adjacency sidecars for graph edge tables | Experimental | Cassie-specific |
| Column-store | USING column indexes, compressed column batches, covered scan acceleration, segment pruning | Stable/Experimental | Cassie-specific |
| Time-series | timestamp range index metadata and planner selection | Experimental | Cassie-specific |
| Integrity hashes | deterministic row, range, and projection-root hashes with persisted verification metadata | Experimental | Cassie-specific; no separate Merkle index is persisted |

## Constraint Support

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Identity | PRIMARY KEY | Stable/Experimental | PostgreSQL-like |
| Nullability | NOT NULL | Stable/Experimental | PostgreSQL-like |
| Uniqueness | UNIQUE | Stable/Experimental | PostgreSQL-like |
| Validation | CHECK | Stable/Experimental | PostgreSQL-like |
| Defaults | DEFAULT | Stable/Experimental | PostgreSQL-like |
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
| PostgreSQL wire | startup, passwordless startup when auth is disabled, cleartext-password auth when enabled, quote/comment-aware simple-query batches, ordered multi-statement results, extended query, parse, bind, describe, execute, sync, flush, close, simple-query COPY FROM STDIN CSV | Stable/Experimental | PostgreSQL-compatible subset; mixed COPY/BACKUP/RESTORE batches return deterministic `0A000` before execution |
| Pgwire results | row description, data row, command complete, ordered result sequences, SQLSTATE-style error response, one final ready for query per simple-query batch | Stable/Experimental | OID-registered binary codecs cover bool, integer widths, float8, bytea, UUID, date/time/timestamp, text-compatible types, and JSON; vector/array/unregistered binary forms return `0A000` |
| Pgwire compatibility | prepared statements, portals, text and OID-registered binary formats, catalog introspection, shared semantic error mapping for malformed SQL, missing relations, missing schemas, unsupported features, deadlines, auth failures, admission exhaustion, and retryable-storage failures | Stable/Experimental | PostgreSQL-compatible subset; common SQLSTATE inventory is documented, not full PostgreSQL parity |
| Role access | authenticated administrators retain the supported SQL surface; authenticated non-admin roles are read-only and may use SELECT, EXPLAIN SELECT, SHOW, SET, and transaction control | Stable baseline | SQLSTATE `42501` and HTTP 403 for forbidden statements; no GRANT/capability SQL |
| HTTP | SQL query, search query, vector query, hybrid query, document APIs, admin manifest export and consistency-check APIs, shared semantic error mapping, and opaque cookie-session admin auth | Stable/Experimental | Cassie REST API |
| Recovery | v2 local snapshot manifests with `cassie-snapshot-manifest.json`, schema/data epochs, per-collection generations, copy-consistency and manifest compatibility validation, copied Midge data directory, restore-to-empty validation of epochs/generations/projections and recovery journals/debt, and cleanup of failed copy targets | Experimental | Cassie-specific local recovery; v1 and every other non-v2 manifest are rejected; no remote orchestration or replication |
| Observability | EXPLAIN, EXPLAIN ANALYZE, query stats, operator stats, cost-model diagnostics, index used, index feedback marker, operator feedback state/reason/cost/confidence diagnostics, adaptive decision/alternative/guard diagnostics, runtime operator switch candidate/pair/threshold/reason diagnostics, join strategy/key/sort/vectorized/fallback diagnostics, time-series index diagnostics, column-batch index used, storage-mode diagnostics, aggregate acceleration, rollup rewrite selected, mixed execution stages, analytical projection markers, rows scanned | Experimental | PostgreSQL-like entry points with Cassie output |
| Projection operations | active version, source checkpoint, lag, freshness, rebuild state, verification state, root state, last replay batch, last error, version state | Experimental | Cassie-specific |
| Operational scale | local node identity, projection ownership, tenant routing hints, partition assignment metadata, generation/state, `pg_catalog.pg_operational_assignments`, external route/drain/move contract | Experimental | Cassie-specific metadata for external orchestration; no distributed query behavior |
| Metrics | latency, throughput, errors, storage-family operation counts, cache occupancy, adaptive candidate, adaptive plan, and runtime operator switch counters, join execution/strategy/row/vectorized batch/spill-fallback counters, projection replay/build/swap/stale/hash/verification/integrity/consistency/mixed-fallback counters, retention enforcement/delete/skip counters, rollup refresh/rewrite/fallback counters, time-series scan/bucket/fallback counters, column-batch scan/fallback/byte/segment/column counters, aggregate acceleration counters | Experimental | Cassie-specific |
| Capacity management | advisory sizing guide, capacity signals, operator thresholds, manual benchmark workflow, and local transport connection caps | Experimental | Cassie-specific operational documentation; no capacity-based admission control or distributed movement |

## Projection Verification Surfaces

- Cassie-owned Midge keys use the clean-break lexkey v5 storage layout. `cf0` stores the global catalog, `cf1` stores temporary state, and each logical database owns one opaque persistent `db-*` family. Existing v4 and older Midge directories, flat or slash-delimited row keys, `doc:` legacy keys, or `__cassie__` key families are intentionally incompatible and must be recreated before startup; Cassie does not attempt in-place migration. See [Per-Database Midge Column Families](database-families.md).
- Fresh startup bootstraps the configured default database plus persisted `public`; `pg_catalog.pg_database` and `information_schema.schemata` expose the live database/schema surface for the current session database.
- Unqualified relation names resolve through the session `search_path` inside the current database only. Cross-database `database.schema.relation` references remain unsupported.
- `CASSIE_OPERATOR_FEEDBACK_ENABLED=1` enables experimental operator-selection feedback. When unset, the planner stays on the deterministic base path and EXPLAIN reports feedback as ignored or disabled.
- `CASSIE_ADAPTIVE_EXECUTION_ENABLED=1` enables experimental adaptive selection among prevalidated read-operator alternatives. `CASSIE_ADAPTIVE_MIN_COST_SAVINGS_BPS` controls the minimum observed savings required before an adaptive alternative replaces the base operator. `CASSIE_ADAPTIVE_MIN_CONFIDENCE_BPS` optionally requires a minimum operator-feedback confidence score before adaptive selection can pass; it defaults to `0`.
- `CASSIE_OPERATOR_SWITCHING_ENABLED=1` enables experimental runtime switching for explicitly prevalidated switch pairs. The first supported pair is `vectorized_join_to_merge_join`, which replays left/right join inputs before emitting rows when `CASSIE_OPERATOR_SWITCH_JOIN_ROW_THRESHOLD` is exceeded.
- REST admin auth uses server-owned opaque `cassie_session` cookies issued by login/current-session/logout endpoints; password-bearing `Authorization` headers are rejected. Pgwire retains its protocol-native credential flow, while both interfaces share credential validation and role lookup.
- REST HTTP transport rejects request bodies over 8 MiB, limits HTTP/1 header buffering to 32 KiB,
  and applies a 10-second header-read deadline; additional idle/request timeout and security-header
  hardening remains in progress. API responses emit `no-store`, `nosniff`, frame-deny,
  no-referrer, and baseline CSP headers; body collection and route execution have a 30-second
  deadline with deterministic 408 responses; state-changing API requests require
  `application/json` and return 415 for other media types.
- Authenticated non-admin roles are read-only across pgwire and the REST SQL routes. DML, COPY, DDL, role/routine administration, projection lifecycle/repair, retention, and operational commands fail before planning or execution. Public embedded sessions created with `CassieSession::new` or `Cassie::create_session` remain trusted in-process callers.
- Hosted embedding providers use provider-specific `CASSIE_VOYAGE_*` and `CASSIE_COHERE_*` settings. `CASSIE_LOCAL_MODEL` and `CASSIE_LOCAL_DIMENSIONS` enable the deterministic local provider for tests, development, and explicit local-only deployments.
- `CREATE TABLE ... WITH (storage = column_store)` creates a column-store table. `pg_catalog.pg_table_storage` and EXPLAIN `storage_mode` expose the effective table mode (`row-store`, `column-indexed`, or `column-store`).
- `CASSIE_VECTORIZED_JOINS_ENABLED=1` enables the experimental vectorized inner/left equi-join executor path. `CASSIE_VECTORIZED_JOIN_BATCH_SIZE` bounds probe batch size, and EXPLAIN reports `vectorized_join_candidate`, `vectorized_join_enabled`, `vectorized_join_batch_size`, and `vectorized_join_fallback_reason`.
- Schema changes publish a new schema epoch for new queries while destructive table, index, view, and column cleanup is deferred until older active query epochs drain.

- `VERIFY PROJECTION <name> [VERSION <version_id>] [MODE metadata_only|hashes_only|indexes_only|full]` runs a read-only local integrity check and persists the latest report.
- `DIFF PROJECTION <left> [VERSION <version_id>] WITH <right> [VERSION <version_id>] [LIMIT n] [AFTER cursor]` returns deterministic local hash differences or an explicit unverifiable result.
- `COMPARE PROJECTION <name> [VERSION <version_id>] WITH MANIFEST '<json>'` compares the current local root digest with an imported manifest digest.
- `PLAN REPAIR PROJECTION <name> [VERSION <version_id>] SCOPE row|range|index|projection-version|full-rebuild` returns a deterministic admin dry-run plan from the latest local integrity report. The plan includes scope, affected-count metadata, the intended action, whether Cassie can execute it locally, and the required follow-up verification command.
- `REPAIR PROJECTION <name> [VERSION <version_id>] SCOPE row|range|index|projection-version|full-rebuild` executes only after a persisted repairable integrity report. Row/range repairs rebuild hashes; index repair rebuilds verified local index sidecars; projection-version repair rebuilds an explicitly named materialized version without activating it; and full-rebuild refreshes the active materialized projection while gating its source and output collections. Cassie immediately runs `VERIFY PROJECTION ... MODE full`, persists an audit report, and requires the version-specific scope to name the version under repair. See [Projection Repair Runbook](projection-repair-runbook.md) for the operator workflow.
- `POST /api/v1/admin/projections/{projection}/verification-manifest` exports an authenticated versioned verification manifest with instance, schema, source checkpoint, hash metadata, root/range summaries, optional row-hash summaries, generated/expiration timestamps, and a canonical manifest digest. It excludes row values, vectors, text bodies, bind values, and credentials.
- `POST /api/v1/admin/projection-consistency-checks` imports two or more manifests and persists a deterministic offline report with `consistent`, `divergent`, `stale`, `incompatible`, or `unverifiable` state. This is an admin workflow only; query planning and execution do not wait on remote manifest checks.
- `pg_catalog.pg_projection_hashes` exposes row/range/root hash state, algorithm metadata, coverage counts, and root digest.
- `pg_catalog.pg_projection_operations` exposes freshness, rebuild, active-version, verification, and root state.
- `pg_catalog.pg_projection_integrity_reports` exposes the latest local integrity report.
- `pg_catalog.pg_projection_repair_reports` exposes persisted local repair audit records, including source integrity counts, scope, action, executable flag, verification requirement, and post-verification state.
- `pg_catalog.pg_projection_comparison_reports` exposes persisted local-vs-manifest comparison reports after restart hydration.
- `pg_catalog.pg_projection_consistency_reports` exposes persisted multi-instance consistency reports, including manifest count, instance ids, mismatch counts, stale/incompatible/unverifiable counts, and deterministic diagnostic samples after restart hydration.
- `pg_catalog.pg_maintenance_debt` exposes persisted derived-state debt by collection and artifact, including target generation, retry count, redacted last error, and the current `maintenance_pending` fallback reason for column batches, projection hashes, rollups, and materialized analytical projections.
- `pg_catalog.pg_operational_assignments` exposes local assignment metadata for external node, tenant, partition, and projection routing. Cassie stores and reports this metadata and documents route/drain/move semantics, but does not route, forward, fan out, or filter queries from it.
- `Cassie::create_snapshot_from_data_dir` and `Cassie::restore_snapshot` provide local snapshot/restore admin APIs around a Cassie manifest and copied Midge data directory. External tooling owns scheduling, transport, retention, encryption, and failover routing.
- `Cassie::begin_database_backup` and `Cassie::begin_database_restore` provide checksummed, logical per-database image streams; pgwire exposes them as `BACKUP DATABASE ... TO STDOUT` and `RESTORE DATABASE ... FROM STDIN`.
- Repair is admin-only and never automatic in query planning or execution. It is local to the Cassie instance and does not imply distributed replication, quorum, remote mutation, or cross-node reconciliation.
- EXPLAIN includes `cost_model`, `selected_cost`, `rejected_alternatives`, `operator_feedback`, `operator_feedback_reason`, `operator_feedback_base_candidate`, `operator_feedback_selected_candidate`, `operator_feedback_base_cost`, `operator_feedback_adjusted_cost`, `operator_feedback_confidence_bps`, `operator_feedback_age_ms`, `operator_feedback_samples`, `operator_feedback_outliers`, `adaptive_plan_enabled`, `adaptive_decision_point`, `adaptive_candidates`, `adaptive_base_alternative`, `adaptive_selected_alternative`, `adaptive_guard`, `adaptive_guard_passed`, `adaptive_reason`, `adaptive_diagnostic`, `operator_switch_candidate`, `operator_switch_enabled`, `operator_switch_pair`, `operator_switch_threshold`, `operator_switch_reason`, `join_strategy`, `join_keys`, `join_sort_required`, `join_fallback_reason`, `vectorized_join_candidate`, `vectorized_join_enabled`, `vectorized_join_batch_size`, `vectorized_join_fallback_reason`, `mixed_execution`, `mixed_stages`, `exact_baseline`, `analytical_projection`, `projection_freshness`, and graph `access_path=graph_adjacency` diagnostics for mixed search/vector/analytical/graph plans.

## Compatibility Notes

- See [PostgreSQL Compatibility](postgres-compatibility.md) for supported, unsupported, and intentionally different PostgreSQL behavior.
- See [Product Roadmap](product-roadmap.md) for milestone grouping and remaining roadmap themes.
- See [Production Readiness](production-readiness.md) for feature-family readiness, evidence, operational signals, restart coverage, and blockers.
- See [Capacity Management](capacity-management.md) for advisory capacity signals, thresholds, and operator actions.
- See [Definition of Done](definition-of-done.md) for how a feature moves from implemented to stable or production-ready.
