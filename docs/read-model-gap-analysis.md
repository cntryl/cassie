# Read-Model Gap Analysis

Report date: 2026-06-21

## Mission Baseline

Cassie should be evaluated as a high-performance read-model database for event-sourced systems. The system of record is the event stream; Cassie materializes, queries, searches, analyzes, and serves projections derived from that stream.

The guiding question is whether a change makes Cassie better at deterministic projection construction, replay-safe rebuilds, high-performance read serving, integrated retrieval, analytical execution, PostgreSQL-compatible access, and operational transparency.

This analysis treats PostgreSQL compatibility as an access and tooling layer. Missing PostgreSQL features are gaps only when they weaken read-model workflows, client access, diagnostics, or operational safety.

Feature prioritization is driven by read-model requirements rather than database taxonomy. If a capability is needed to build, operate, analyze, search, report on, or serve projections, it is in scope regardless of whether it resembles relational, analytical, search, vector, or time-series database work.

## Executive Summary

Cassie already has strong foundations for serving rich read models: a PostgreSQL wire interface, broad SQL coverage, Midge-backed row blobs, scalar/full-text/vector/hybrid indexes, column-batch scans, rollups, retention work, metrics, EXPLAIN, and benchmarks across hot paths and system tiers.

The largest gap is that the product surface is still organized mostly as a SQL-over-document database with PostgreSQL-like feature breadth. The event-sourced read-model lifecycle is present as scaffolding and roadmap issues, but it is not yet the organizing contract for ingestion, replay, rebuild, versioning, verification, freshness, and operations.

The most important correction is to promote projection lifecycle and verification work from later analytical/advanced roadmap items into core read-model infrastructure. Materialized projections, projection versioning, projection swaps, row hashes, Merkle roots, rebuild verification, and integrity verification are currently open issues with low `P3` priority, but they are central to Cassie's stated role.

## Current Strengths

- **Broad query access exists.** `docs/feature-support.md` and `docs/product-roadmap.md` describe implemented SELECT, DML, joins, CTEs, set operations, windows, DDL, catalog, and pgwire behavior. This supports existing tools and reporting workflows.
- **Midge remains the direct storage layer.** `AGENTS.md`, `docs/module-organization.md`, and storage code keep row blobs authoritative and avoid a second storage abstraction.
- **Search and retrieval are differentiated.** Full-text, vector, hybrid scoring, pgvector-style operators, embedding-provider validation, and scored executor paths are present across `src/search/`, `src/vector/`, `src/hybrid/`, `src/embeddings/`, and integration tests.
- **Analytical acceleration is underway.** Column batches, aggregate acceleration, rollups, `time_bucket`, and retention policies are documented and represented in parser, catalog, executor, metrics, and tests.
- **Operational signals exist.** Runtime metrics, EXPLAIN output, cardinality feedback, column-batch fallback counters, rollup freshness/fallback metrics, pgwire metrics, and startup diagnostics exist across `src/runtime*`, `tests/metrics_*`, `tests/column_batches.rs`, and `tests/time_series_rollups.rs`.
- **Performance work has structure.** The tiered benchmark suite covers row codec, keys, predicates, BM25, vector distance, ingest, search, vector, hybrid, executor, query, rebuild, startup, mixed load, pgwire, and HTTP paths.

## P0 Gaps

### 1. Product Docs Do Not Yet Lead With The Read-Model Contract

Evidence:

- `docs/README.md` currently introduces Cassie as a SQL-over-document-store database engine.
- `docs/product-roadmap.md` leads with SQL foundation, schema/catalog, indexing, search/AI, analytics, and PostgreSQL compatibility.
- The roadmap lists transactions and savepoints as implemented/stable, which is technically useful, but the docs do not clearly frame them as projection-state mutation tools rather than OLTP positioning.
- `docs/postgres-compatibility.md` correctly says full PostgreSQL equivalence is not the goal, but the read-model/event-stream mission is not the top-level product frame.

Impact:

Readers can infer that PostgreSQL-like completeness is a primary goal. That weakens prioritization and can make OLTP-style gaps look more important than projection lifecycle gaps.

Recommendation:

- Add a first-class "Read-Model Posture" section to product docs.
- Reframe SQL and pgwire as access/tooling surfaces for projections.
- Reclassify DML and transaction language around projection mutation, replay batches, and operational correction workflows.
- Make roadmap sections start with projection lifecycle, rebuild/replay safety, verification, retrieval, analytics, and operations before PostgreSQL compatibility.

### 2. Projection Ingestion And Replay Contract Is Not First-Class

Evidence:

- `src/catalog/collections.rs` defines `ProjectionMeta` with `collection`, `schema_version`, `offset`, `lag`, and `rebuild_state`.
- `tests/midge_metadata_stats.rs` proves persistence and hydration of this metadata.
- There is no visible first-class event-stream source model: stream identity, event position, checkpoint, event sequence, replay batch id, idempotency key, projection definition fingerprint, source schema epoch, or deterministic handler contract.
- Existing ingestion paths are document/table oriented: SQL DML, REST document APIs, and benchmark helpers such as `benches/support/workloads/system.rs`.

Impact:

Cassie can store and query projection rows, but it does not yet define how an event-sourced system proves a projection was built deterministically from a specific event stream position. Offset and lag fields exist, but their semantics are not tied to event-stream identity or replay safety.

Recommendation:

- Define a projection source contract with source stream id, projection id, source position/checkpoint, schema epoch, replay run id, and idempotency semantics.
- Add ingestion/replay APIs that update projection rows and projection metadata atomically enough for Cassie's supported durability model.
- Expose checkpoint, lag, rebuild state, last applied event, and replay diagnostics in catalog views and metrics.
- Add TDD coverage for idempotent replay, duplicate event handling, out-of-order event rejection or quarantine, restart hydration, and partial replay recovery.

### 3. Projection Versioning, Swaps, And Materialization Are Under-Prioritized

Evidence:

- Phase 01 now implements materialized projections, projection versioning, and active-version swaps as experimental Cassie-specific lifecycle features.
- `docs/feature-support.md` and `docs/product-roadmap.md` mark the projection lifecycle surface as implemented and experimental.
- Verification gates for safe swaps still depend on phase 02 row hash and rebuild verification work.

Impact:

Fast rebuilds and replay safety require building a new projection version without corrupting the active read model, verifying it, and atomically switching readers. That is not an advanced analytics concern; it is core infrastructure for event-sourced read models.

Recommendation:

- Keep the implemented v1 lifecycle experimental until phase 02 verification can gate safe activation.
- Tie verification state into active-version swaps, cache invalidation, catalog diagnostics, metrics, and pgwire-visible errors.

### 4. Rebuild Verification And Merkle Work Are Core, Not V5 Advanced Work

Evidence:

- `issues/phase-02/issue-01.md` covers row hashes.
- `issues/phase-02/issue-02.md` covers range hashes.
- `issues/phase-02/issue-03.md` covers projection Merkle roots.
- `issues/phase-03/issue-05.md` covers projection diffing.
- `issues/phase-02/issue-04.md` covers rebuild verification.
- `issues/phase-02/issue-06.md` covers projection integrity verification.
- These are open and currently framed under V5 Verification & Advanced Execution, mostly with `Priority: P3`.

Impact:

Without row-level and projection-level verification, Cassie cannot confidently answer whether a rebuilt projection matches the source-derived expected state. This limits replay safety, deterministic rebuild claims, multi-instance validation, repair diagnostics, and operational trust.

Recommendation:

- Promote row hashes, projection roots, and rebuild verification to the core read-model roadmap.
- Make hash availability optional for query correctness, but required for "verified rebuild" and "safe swap" status.
- Sequence the work as row hash -> range/root hash -> rebuild verification -> projection diff -> integrity verification.
- Add operational diagnostics before distributed comparison or repair workflows.

## P1 Gaps

### 5. Freshness Semantics Are Fragmented

Evidence:

- Rollups have explicit `lag_rows`, stale state, fallback, EXPLAIN, and metrics in `src/catalog/rollups.rs`, `src/executor/execution/rollups.rs`, and `tests/time_series_rollups.rs`.
- Projection metadata has `offset`, `lag`, and `rebuild_state`, but no comparable user-facing freshness contract.
- Column batches and aggregate acceleration expose fallback metrics, but they are not tied to projection freshness.
- Retention policies are in active local changes and documented as explicit enforcement, but their interaction with projection checkpoints and rebuild verification is not yet defined.

Impact:

Read-model users need a consistent answer to "is this projection fresh enough to serve?" Today that answer differs by feature area.

Recommendation:

- Define one freshness model for projections and derived accelerators: fresh, stale, rebuilding, failed, unverifiable, and unknown.
- Require stale or unverifiable derived state to fall back to row blobs/source projection rows where semantics permit.
- Surface freshness through catalog views, metrics, EXPLAIN, and admin diagnostics.

### 6. Ingestion And Rebuild Benchmarks Need Read-Model Acceptance Targets

Evidence:

- `benches/tier2_subsystem_ingest.rs` measures `projection_write_path`.
- `benches/tier3_system_rebuild.rs` measures `projection_rebuild_query` and `index_rebuild_ddl`.
- `benches/tier3_system_mixed_load.rs` measures mixed ingest/query.
- `docs/indexes-and-constraints.md` has concrete index benchmark expectations, but there are no comparable projection replay/rebuild targets.

Impact:

Cassie has benchmark coverage, but product claims around fast projection ingestion and rebuilds need explicit budgets and scale points.

Recommendation:

- Add benchmark targets for event replay throughput, idempotent duplicate handling, projection rebuild from row blobs, rebuild verification, version swap latency, and lag catch-up time.
- Record targets for 10k, 100k, and 1M row/event scales where practical.
- Keep benchmarks tied to row blobs and Midge so they validate the intended architecture.

### 7. Mixed Search/Vector/Analytics Planning Is Still A Later Roadmap Item

Evidence:

- Search, vector, hybrid, column-batch, rollup, and aggregate acceleration exist individually.
- `issues/phase-02/issue-08.md` covers mixed search/vector/analytical execution with exact final results, stale fallback, stage diagnostics, and metrics.
- That issue is open and `Priority: P3`.

Impact:

Event-sourced read models often power dashboards, operational search, and AI retrieval in the same workflow. Cassie's differentiator is the combination of these capabilities, so exact mixed execution is a product-level gap.

Recommendation:

- Raise mixed retrieval/analytics planning to P1.
- Define exactness rules for candidate generation, scalar filtering, vector/text scoring, grouping, ordering, offset, and limit.
- Require EXPLAIN and metrics to identify candidate counts, exact scoring rows, aggregate groups, selected accelerators, freshness, and fallback reasons.

### 8. Operational Transparency Needs A Projection-Centric View

Evidence:

- Runtime metrics are broad and useful.
- `docs/feature-support.md` lists projection lag metrics, retention counters, rollup counters, column-batch counters, and aggregate acceleration counters.
- `src/catalog/virtual_views.rs` exposes rollup information, but there is no clear general projection operations view covering checkpoint, lag, source position, version, rebuild, freshness, verification, and last error.

Impact:

Operators need to diagnose "what projection am I serving, from what event position, with what freshness and verification state?" Existing metrics answer parts of this, but not as a unified operational surface.

Recommendation:

- Add projection-centric catalog/admin views for source position, active version, rebuild state, lag, freshness, verification state, last replay batch, last error, and stale accelerator counts.
- Make `EXPLAIN` show when a query reads an active projection version, derived rollup, column batch, or fallback path.
- Add smoke tests for restart/hydration of projection operations metadata.

### 9. PostgreSQL Compatibility Needs A Read-Model Client Matrix

Evidence:

- `docs/postgres-compatibility.md` names psql, sqlx, diesel, prisma, SQLAlchemy, and migration tools as a matrix that should grow around real clients.
- `tests/compatibility_matrix.rs` exists, but the docs still describe broader client compatibility work as planned.

Impact:

Pgwire compatibility matters because read models should be accessible to existing tools. The gap is not PostgreSQL feature parity; it is confidence that common clients can query projections, inspect schemas, prepare statements, and handle Cassie-specific features predictably.

Recommendation:

- Maintain a client matrix focused on read-model use cases: query, prepared statements, catalog introspection, migrations/schema probes, analytics/reporting, and error handling.
- Explicitly mark unsupported PostgreSQL features as out of scope when they are OLTP-specific and irrelevant to projection serving.

## P2 Gaps

### 10. Roadmap Priorities Still Reflect Feature Breadth More Than Mission Criticality

Evidence:

- Many central read-model items are P3: materialized projections, versioning, swaps, row hashes, Merkle roots, rebuild verification, integrity verification, analytical projections, mixed execution.
- Implemented SQL feature breadth is prominent in docs and roadmap.

Impact:

Priority labels may steer engineering toward breadth and advanced execution before lifecycle guarantees.

Recommendation:

- Reprioritize open issues by read-model critical path:
  - P0: projection source/checkpoint contract, materialized projections, versioning, verified swaps.
  - P1: row hashes, Merkle roots, rebuild verification, projection operations views, replay/rebuild benchmarks.
  - P2: distributed comparison, advanced mixed execution, column-native tables, vectorized joins/aggregates.

### 11. File-Size Constraints Are Close To Their Limits

Evidence:

- The file-size audit shows several source files close to 1,000 lines: `src/planner/physical.rs` at 997, `src/sql/binder/schema.rs` at 989, `src/app/documents.rs` at 988, `src/runtime.rs` at 985, `src/sql/parser/schema.rs` at 981, `src/executor/execution/scored.rs` at 976, and `src/executor/execution/aggregate_exec.rs` at 975.
- Several integration tests are also large, including `tests/integration_sql_transactions.rs` at 939 and `tests/parser_indexes.rs` at 919.

Impact:

Projection lifecycle work will touch parser, binder, planner, executor, storage, catalog, runtime, and tests. Without extraction first, new work is likely to violate `AGENTS.md` and `docs/module-organization.md`.

Recommendation:

- Add extraction tasks before broad projection lifecycle work.
- Create focused modules for projection materialization/versioning, verification metadata, replay metadata, and projection operations diagnostics.
- Create focused test files instead of growing large integration files.

### 12. Test Style Has Known Violations

Evidence:

- `rg '#\[tokio::test\]' tests src -g '*.rs'` finds async tests in `tests/executor_limits.rs` and `tests/executor_commands.rs`.
- `AGENTS.md` requires current-thread Tokio runtime builder tests instead of `#[tokio::test]`.

Impact:

This is not a product gap, but it increases friction for future validation and can block clean close-out of projection work.

Recommendation:

- Convert existing `#[tokio::test]` cases before extending those files.
- Keep new projection tests in the required `should_` style with `// Arrange / Act / Assert`.

### 13. Dirty Worktree Makes Baseline Classification Noisy

Evidence:

- `git status --short` shows modified docs, issues, runtime/catalog/executor/sql files, and untracked retention files including `src/catalog/retention.rs`, `src/executor/execution/retention.rs`, `src/runtime/retention_metrics.rs`, `src/sql/parser/retention.rs`, and `tests/time_series_retention.rs`.
- `git diff --stat` shows active changes across retention, catalog, runtime, parser, binder, executor, Midge metadata, and docs.

Impact:

Some retention/time-series claims may be in progress rather than validated committed baseline. A gap analysis should not treat uncommitted work as production-ready.

Recommendation:

- Track retention as in-progress until built, tested, documented, and validated.
- Re-run this gap analysis after the retention work is merged or intentionally abandoned.

## Recommended Roadmap Corrections

1. Add a top-level roadmap theme: **Projection Lifecycle And Replay Safety**.
2. Move materialized projections, projection versioning, projection swaps, row hashes, projection roots, and rebuild verification into the near-term roadmap.
3. Add a new issue for the missing event-stream projection source/checkpoint contract.
4. Add a new issue for projection operations views and metrics.
5. Reclassify PostgreSQL compatibility work as "read-model client interoperability" and focus it on practical client workflows.
6. Add explicit performance targets for projection ingestion, replay, rebuild, verification, and swap.

## Suggested New Issues

- **Projection Source Checkpoints**: store source stream identity, source position, replay batch id, last event id, lag, and failure diagnostics with each projection.
- **Idempotent Replay Ingestion**: define duplicate-event behavior, out-of-order handling, batch atomicity, and restart recovery for projection writes.
- **Projection Operations Views**: expose active version, source position, lag, rebuild state, freshness, verification state, last error, and fallback counters.
- **Projection Rebuild Performance Targets**: establish benchmark scenarios and acceptance thresholds for replay, rebuild, verification, and swap.
- **Read-Model Product Posture Docs**: update docs to make event-sourced read-model use the first organizing principle.

## Acceptance Criteria For Closing The Strategic Gap

Cassie can claim the read-model database role when:

- A projection has a durable source/checkpoint identity tied to an event stream or replay source.
- Replaying the same event stream into the same projection definition produces the same logical rows.
- A new projection version can be built while the previous version remains readable.
- Rebuilt versions can be verified before activation.
- Swaps are atomic from the reader's perspective and have rollback-capable metadata.
- Query planning never silently uses stale derived state.
- Operators can see projection lag, freshness, active version, rebuild status, verification status, and fallback reasons.
- Benchmarks report ingestion, rebuild, verification, and query performance at documented scales.

## Immediate Next Steps

1. Update docs/product framing to make the read-model mission explicit.
2. Reprioritize issues 119, 120, 121, 126, 128, 130, and 142.
3. Add the missing projection source/checkpoint issue.
4. Add projection operations catalog/metrics issue.
5. Plan extraction work around near-1,000-line files before implementing lifecycle features.
6. Convert existing `#[tokio::test]` violations in touched test areas.
