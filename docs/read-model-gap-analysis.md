# README Goal Gap Analysis

Report date: 2026-06-23

## Mission Baseline

This analysis evaluates Cassie against the goals in `README.md`.
The README defines Cassie as a purpose-built read-model database for CQRS and event-sourced systems where the event stream is the source of truth and Cassie owns fast, predictable query performance over projections.

The relevant product promises are:

- single-node-first performance and predictable operation
- operational scale through independent nodes, not distributed SQL
- optimized read-model query patterns
- benchmarked and measured performance
- event-sourcing-native replay, rebuild, snapshot, and restore workflows
- simplicity over broad database complexity
- practical PostgreSQL access without full PostgreSQL parity

## Executive Summary

Cassie is much closer to the README mission than the previous gap analysis suggested.
The core SQL engine, projection lifecycle, replay metadata, verification, search, vector, hybrid, analytics, pgwire, metrics, and advanced planner/executor surfaces are implemented and tested at least at baseline or experimental level.

The largest remaining gaps are now production evidence and operational depth gaps, not basic feature-existence gaps:

- Tier 4 operational-scale metadata now covers local assignment inspection, external router/drain/move contracts, local snapshot/restore, and advisory capacity guidance. Actual traffic routing and node movement remain outside Cassie.
- Performance has broad benchmark coverage and an initial 10k/100k manual feedback loop, but larger-scale claims and production-grade capacity thresholds still need follow-up evidence.
- Several important read-model capabilities remain experimental or planned by depth, especially bucket-native time-series storage depth, broader read-path combinations beyond the Phase 09 narrow mixed-order/expression-index proof, byte-accurate capacity reporting, and non-tokio PostgreSQL client probe depth.
- The product boundary around procedures is now explicit: limited experimental compatibility/admin support is allowed, while stored-procedure and trigger-based business-logic platforms remain out of scope.
- The issue backlog has an archived phase surface and Phase 08 now records the README-goal closure baseline.

## Current Strengths

- **Core read-model database:** SELECT, predicates, ordering, pagination, joins, aggregates, windows, DML, DDL, constraints, indexes, catalog views, and pgwire flows are implemented and tested.
- **Projection lifecycle:** projection checkpoints, replay metadata, idempotent replay, materialized projections, versioned builds, active-version swaps, freshness, verification, and projection operations views exist as experimental Cassie-specific surfaces.
- **Verification and consistency:** row hashes, range hashes, projection roots, rebuild verification, integrity reports, projection diffing, manifest comparison, repair planning, local repair audit reports, and offline multi-instance consistency reports are implemented at baseline.
- **Recovery:** local v1 snapshots combine a copied Midge data directory with a Cassie manifest that records schema epoch, projection checkpoint/version, hash metadata, generated timestamp, Cassie version, and compatibility status.
- **Retrieval:** full-text search, vector search, pgvector-style operators, hybrid scoring, HNSW/IVFFlat metadata and execution paths, and embedding-provider validation exist.
- **Analytics:** column batches, segment pruning, aggregate acceleration, `time_bucket`, rollups, retention policies, analytical projection routing, EXPLAIN, and metrics are represented.
- **Operational signals:** `/health`, `/liveness`, runtime metrics, projection metrics, pgwire/rest metrics, EXPLAIN ANALYZE deltas, and catalog diagnostics exist.
- **Performance structure:** tiered Criterion benchmarks cover hot paths, ingest, search, vector, hybrid, executor, query, rebuild, startup, mixed load, pgwire, and HTTP.

## Gap Matrix

| README Goal | Current State | Gap | Priority |
| --- | --- | --- | --- |
| Single-node first | Midge remains the direct storage layer; read/write contracts, benchmarks, 10k/100k manual benchmark scenarios, and advisory capacity guidance exist. | Production claims still need byte-accurate capacity reports, deployment-profile thresholds, and larger-scale evidence. | P1 |
| Operational scale over distributed SQL | Offline manifests explicitly avoid distributed query/replication semantics; local assignment metadata, external routing contracts, local snapshot/restore, and capacity guidance are available. | Deployment-specific router integrations, fleet monitoring thresholds, and production evidence remain outside Cassie. | P1 |
| Purpose-built read models | Primary/secondary lookups, range queries, tenant filtered pages, narrow mixed-order equality-prefix scans, exact expression-index equality seeks, aggregations, search, vector, hybrid, projections, and analytics exist. | Remaining depth is focused on broader read-path combinations, persisted bucket-native time-series storage, and deeper projection-shaped layout guidance. | P1 |
| Performance is a feature | Broad benchmark suite, performance contracts, manifest-owned 10k/100k manual scenarios, and capacity signal guidance exist. | Future work should improve scenario quality, capture repeatable local evidence, add byte-accurate capacity data, and add larger scale points. | P1 |
| Event-sourcing native | Replay batches, checkpoint metadata, duplicate skip ledger, materialized projection builds, handler determinism contracts, replay failure guidance, verification, repair plans, local hash repair, swaps, and local snapshot/restore exist. | Production replay capacity evidence remains classification work. | P1 |
| Simplicity wins | Docs now frame Cassie as a read-model database, reject distributed SQL, and define procedures as limited compatibility/admin support rather than application business logic. | Feature surface is broad and can read like PostgreSQL parity unless non-goals and experimental boundaries stay explicit. | P1 |
| Practical PostgreSQL access | pgwire startup, auth, simple/extended query, prepared statements, catalog probes, SQLSTATE-style errors, a maintained client matrix, default tokio-postgres coverage, and an opt-in psql probe exist. | sqlx/diesel/prisma/SQLAlchemy automation remains future probe depth. | P1 |

## P0 Gaps

### 1. Operational-Scale Orchestration Is Still Incomplete

Evidence:

- `README.md` names workload isolation, projection ownership, tenant routing, partition assignment, and horizontal expansion of independent read nodes.
- Existing docs emphasize offline manifests and explicitly avoid distributed query execution, replication, quorum reads, and repair.
- `docs/operational-scale.md` and `pg_catalog.pg_operational_assignments` now define local assignment metadata and diagnostics.

Impact:

Cassie can run as an independent node, report local assignment claims, and document how external routers consume assignment metadata for route, drain, move, failure, and rollback workflows.
That closes the baseline README distinction between operational scale and distributed SQL without adding distributed query behavior.

Recommendation:

- Treat local assignment metadata as the baseline contract.
- Keep the external router contract as the operational-scale baseline.
- Keep the design outside the query path: no distributed query planning, no consensus, no cross-node reads.
- Keep docs clear that external orchestrators consume Cassie metadata and make routing decisions outside Cassie.
- Treat production router integrations, fleet monitoring thresholds, and deployment-specific evidence as follow-on production-depth work.

### 2. Performance Benchmarks Need Capacity Evidence

Evidence:

- `docs/performance-contracts.md` asks for explicit latency, throughput, freshness, and memory targets.
- Benchmarks exist across tiers, including ingest, rebuild, query, mixed load, pgwire, and HTTP.
- The first manual scenario table covers 10k and 100k core read, replay, rebuild, verification, search/vector/hybrid, pgwire, and HTTP workloads.

Impact:

Cassie now has named local fixtures for developer feedback while changing read-model paths, but these are not production SLA claims and are not automatic CI gates.

Recommendation:

- Improve scenario fidelity as benchmark evidence stabilizes.
- Keep expensive runs as explicit dev-time feedback loops unless a future issue defines a lightweight CI subset.
- Extend evidence before making 1M-scale production claims.
- Keep p50, p95, p99, throughput, memory budget, and fallback counters tied to benchmark ownership.

### 3. Repair Workflows Have A Local Admin Baseline

Evidence:

- Verification, diffing, manifest comparison, and consistency reports exist.
- `PLAN REPAIR PROJECTION` returns deterministic dry-run plans for row, range, index, projection-version, and full-rebuild scopes.
- `REPAIR PROJECTION` executes the safe local row/range hash-rebuild path when the latest integrity report is repairable, audits the action, and immediately verifies the projection.

Impact:

Operators can detect divergence or stale materialization and use Cassie-defined admin commands for repair planning and safe local hash repair.
Index, projection-version, and full-rebuild repair scopes remain deterministic dry-run/error surfaces until their safe mutation behavior is implemented.

Recommendation:

- Keep repair out of automatic query execution.
- Keep repair local, explicit, idempotent, audited, and post-verified.
- Do not add distributed replication, quorum, remote mutation, or query-path repair semantics.

## P1 Gaps

### 4. Read-Path Optimization Has An MVP Baseline

Evidence:

- `issues/phase-06/README.md` archives point lookup, scalar index seek/prefix/range scans, ordered bounded scans, row-id keyset/top-k, EXPLAIN labels, metrics, and benchmark ownership.
- Tenant filtered pages using composite scalar equality-prefix plus range/order fields are covered by integration tests and documented performance contracts.
- Phase 09 issue 04 adds narrow mixed-order equality-prefix proof and exact expression-index equality seeks with EXPLAIN, metrics, restart, and benchmark ownership.
- Broader mixed-direction suffix ordering, expression range scans, and expression ORDER BY lowering remain explicit follow-on depth.

Impact:

The README says Cassie is optimized for primary-key lookups, secondary-index lookups, time-range queries, aggregations, reporting, search, vector, and hybrid search.
The MVP baseline covers the core single-node read-model shapes, but not every advanced secondary-ordering or expression-index combination.

Recommendation:

- Treat the Phase 06 archived scope plus tenant filtered-page coverage as the MVP read-optimization baseline.
- Track broader mixed-direction suffix ordering and expression range/order lowering only as explicit future slices with EXPLAIN assertions and metrics.
- Prioritize tenant-scoped filtered pages, time-range pages, and projection-shaped reads over generic SQL breadth.

### 5. Time-Series Has An MVP Baseline

Evidence:

- `docs/product-roadmap.md` marks time-series index metadata and range planning as implemented baseline.
- Row-backed time-series range scans are selected for timestamp range predicates with EXPLAIN metadata and runtime counters for selected scans, rows, scanned buckets, skipped buckets, last index, and fallbacks.
- Insert/update/delete/restart correctness is tested against authoritative row blobs.
- Retention enforcement uses normal document deletion, refreshes rollups, and marks dependent materialized projections stale for re-verification.
- Manual benchmark scenarios cover time-window scans, retention enforcement, and rollup refresh at 10k and 100k fixture scales.

Impact:

Time-range reads, rollups, and retention interactions are complete enough for an MVP baseline.
Persisted bucket-native membership remains a depth/capacity optimization, not an MVP correctness blocker.

Recommendation:

- Keep row blobs as the source of truth for the MVP path.
- Treat persisted bucket-native storage as a future performance slice with its own migration and fallback proof.
- Use the manual Criterion scenarios as developer feedback before making larger-scale time-series claims.

### 6. PostgreSQL Client Matrix Has A Baseline

Evidence:

- `docs/postgres-compatibility.md` now contains a maintained read-model client matrix for tokio-postgres, psql, sqlx, diesel, prisma, SQLAlchemy, and migration-tool workflows.
- `tests/compatibility_matrix.rs` covers tokio-postgres startup, simple and extended query flows, prepared queries, DDL/DML, `ON CONFLICT`, constraints, SQLSTATE metadata, recursive CTEs, and syntax-error recovery.
- An ignored optional `psql` probe validates non-interactive DDL/DML/query behavior when local `psql` is installed and `CASSIE_RUN_PSQL_COMPAT=1` is set.
- Untested clients are marked planned rather than implied supported.

Impact:

Pgwire compatibility matters because read models should be queryable from ordinary application and reporting tooling.
The baseline now prevents broad unsupported claims, while deeper client-specific automation remains future work.

Recommendation:

- Keep default compatibility tests centered on deterministic tokio-postgres coverage.
- Add sqlx, diesel, prisma, and SQLAlchemy probes only when they can be isolated from default-suite brittleness.
- Keep unsupported OLTP or PostgreSQL-server features intentionally out of scope.

### 7. Procedure Non-Goal Boundary Is Resolved

Evidence:

- `README.md` keeps stored-procedure business-logic platforms and trigger-based business logic out of scope.
- `docs/feature-support.md` describes `CREATE PROCEDURE` and `CALL` as a limited experimental compatibility/admin surface.
- `docs/postgres-compatibility.md` documents unsupported procedural-language expectations, including PL/pgSQL, triggers, dynamic SQL, procedure-local transaction control, recursive workflows, and OLTP business logic.
- Existing tests continue to exercise the limited procedure surface and rejection paths for transaction control and recursion.

Impact:

The current implementation can remain available for simple compatibility/admin workflows without implying a product direction toward stored procedure business logic.

Recommendation:

- Keep procedures experimental and limited.
- Do not add triggers, procedural languages, dynamic SQL, transaction control inside procedures, recursive procedure workflows, or OLTP business-logic semantics.
- Revisit behavior only if Issue 09 production-readiness classification decides to deprecate, narrow, or promote the surface.

### 8. Production-Ready Classification Has A Baseline

Evidence:

- `docs/definition-of-done.md` defines production-ready as stable plus benchmark or operational evidence.
- `docs/production-readiness.md` now records owner, support level, readiness, evidence, benchmark/operational signals, restart coverage, and blockers for major feature families.
- The matrix explicitly avoids marking any feature family production-ready by default.
- Stable areas are listed as production-ready candidates only after declared deployment-profile evidence is complete.

Impact:

Users can distinguish implemented/stable behavior from production-ready commitments without broad, unsupported promotion claims.

Recommendation:

- Keep production-ready classification separate from implementation status.
- Promote feature families only when the linked evidence and blockers in `docs/production-readiness.md` support the claim.
- Use Issue 10 to add capacity guidance and reconcile stale documentation before making stronger production claims.

## P2 Gaps

### 9. Capacity Management Has A Documented Baseline

Evidence:

- `README.md` lists capacity management under Tier 4 operational scale.
- [Capacity Management](capacity-management.md) now defines CPU, memory, disk, index overhead, projection count, tenant load, rebuild pressure, cache occupancy, and fallback-rate signals.
- Runtime metrics expose storage-family operation counts, cache occupancy, projection write/rebuild counters, column-batch byte totals, fallback counters, retention/rollup/time-series counters, and pgwire/rest blocking elapsed counters.
- EXPLAIN, catalog views, operational assignments, production-readiness classification, and manual benchmark scenarios are linked into a sizing workflow.

Impact:

Operators have Cassie-specific guidance for manual sizing and capacity triage.
The baseline is advisory and does not yet provide automatic admission control, byte-accurate storage-family accounting, or production thresholds by deployment profile.

Recommendation:

- Treat the capacity guide as the MVP operator baseline.
- Add byte-accurate data/index/full-text/vector/column-batch family reports only through a future diagnostics issue.
- Promote capacity claims only after a deployment profile records benchmark targets, host shape, workload mix, and alert thresholds.

### 10. Documentation Goal Levels Are Reconciled

Evidence:

- `README.md` is product-level and principle-driven.
- `docs/feature-support.md` is detailed and current.
- `docs/product-roadmap.md` separates roadmap status from production-readiness classification.
- `docs/read-model-autopilot-plan.md` is archived as a rebaseline execution artifact instead of the live gap list.
- `docs/capacity-management.md`, `docs/production-readiness.md`, and this gap analysis now separate implemented baseline, experimental guidance, planned depth, and production blockers.

Impact:

Readers can follow README for mission, feature support for behavior, roadmap for implementation status, production readiness for evidence, and this gap analysis for remaining deltas.

Recommendation:

- Keep `README.md` as product mission, `feature-support.md` as feature truth, `product-roadmap.md` as status, `production-readiness.md` as evidence classification, and gap analysis as the current delta.
- Avoid phase-history language in product-facing docs unless it explains an archived contract.

### 11. Large Files Limit Future Work

Evidence:

- The file-size audit shows several source and test files near or over the 1,000-line limit, including `src/app/documents.rs`, `src/executor/executor.rs`, and `src/sql/parser/schema.rs`.
- `AGENTS.md` requires extraction before adding substantial feature work to oversized files.

Impact:

Operational-scale, snapshot/restore, read-path, and capacity work will touch broad modules.
Without extraction, implementation work will either violate repo rules or become harder to review.

Recommendation:

- Add extraction issues before broad operational-scale or snapshot/restore implementation.
- Split by ownership: routing/ownership metadata, snapshot/restore admin flows, capacity diagnostics, and read-path proof modules.

## Phase 08 Issue Backlog

Phase 08 tracks the README-goal gaps in execution order:

Closed baseline:

- [Operational Scale](operational-scale.md): local assignment metadata, restart hydration, catalog diagnostics, external router/drain/move contracts, rollback semantics, and capacity movement guidance.
- [Snapshot And Restore](snapshot-restore.md): local Midge-directory snapshot bundle, Cassie compatibility manifest, restore validation, and query-after-restore smoke coverage.
- [Performance Contracts](performance-contracts.md): manifest-owned 10k/100k manual benchmark scenarios for core read, replay, rebuild, verification, search/vector/hybrid, pgwire, and HTTP workloads.
- [Feature Support](feature-support.md): projection repair dry-run commands, local row/range hash repair, persisted repair audit reports, and admin-only/no-distributed repair boundaries.
- [Performance Contracts](performance-contracts.md): read-optimization MVP baseline for point lookup, scalar index seek/prefix/range scans, ordered bounded scans, row-id keyset/top-k, and tenant filtered pages.
- [Performance Contracts](performance-contracts.md): time-series MVP baseline for row-backed range scans, bucket diagnostics, retention freshness effects, rollup refresh, and manual 10k/100k feedback benches.
- [PostgreSQL Compatibility](postgres-compatibility.md): maintained read-model client matrix, default tokio-postgres compatibility coverage, opt-in psql probe, and explicit planned/unsupported client boundaries.
- [Feature Support](feature-support.md): procedure boundary resolved as limited experimental compatibility/admin support, with stored-procedure and trigger-based business logic explicitly out of scope.
- [Production Readiness](production-readiness.md): feature-family readiness matrix with owners, support levels, evidence, benchmark/operational signals, restart coverage, and blockers.
- [Capacity Management](capacity-management.md): advisory sizing signals, operator thresholds, cache/index/rebuild/fallback guidance, and manual benchmark workflow.

Remaining sequence:

- None for Phase 08 README-goal closure.

## Phase 09 Follow-On Backlog

Phase 09 tracks planned or planned-by-depth work after README-goal closure:

Closed baseline:

- [Performance Contracts](performance-contracts.md): deployment-profile benchmark reports, larger fixture placeholders, and production-readiness evidence boundaries without unsupported SLA claims.
- [Module Organization](module-organization.md): extraction gate lowered the immediate Midge, executor, and schema-parser touch points below the 1,000-line file limit before read-path, projection, and time-series depth work.
- [Read-path depth](performance-contracts.md): narrow equality-prefix mixed ordering and exact expression-index equality seeks with EXPLAIN proof, metrics, restart coverage, and manual benchmark ownership.
- [Projection replay contracts](projection-replay-contracts.md): handler-owned determinism, Cassie-owned replay metadata, duplicate/conflict handling, failure observability, restart hydration, and safe rebuild/verify/swap guidance.

Remaining sequence:

- [Persisted bucket-native time-series storage](../issues/phase-09/issue-06.md): bucket-native metadata, mutation/restart correctness, retention interactions, and fallback proof.
- [Pgwire client probes](../issues/phase-09/issue-07.md): opt-in non-tokio client probes while keeping the default suite deterministic.
- [Byte-accurate capacity diagnostics](../issues/phase-09/issue-08.md): local byte accounting for storage families, indexes, sidecars, and rebuild artifacts where feasible.
- [Repair depth and runbooks](../issues/phase-09/issue-09.md): operator runbooks and the next safe local repair scope.
- [Adaptive planning depth](../issues/phase-09/issue-10.md): guarded adaptive planning improvements with promotion gates and fallback diagnostics.
- [Experimental promotion criteria](../issues/phase-09/issue-11.md): evidence requirements for catalog, procedure, rollup, HNSW, embedding, and related experimental surfaces.

## Acceptance Criteria For README Alignment

Cassie fully satisfies the README goals when:

- A single node has documented performance feedback loops and advisory capacity guidance for core read-model workloads.
- Independent nodes can be assigned projection/tenant/partition ownership without adding distributed query semantics.
- Operators have documented routing, ownership, snapshot, restore, health, and capacity workflows.
- Replay, rebuild, verification, repair, and swap workflows are deterministic, observable, and tested after restart.
- Query planning exposes optimized and degraded paths for supported read-model query shapes.
- Search, vector, hybrid, analytics, and time-series paths have exactness/fallback documentation and benchmark evidence.
- PostgreSQL compatibility is validated through a client matrix focused on read-model use cases.
- Non-goals are consistently enforced across README, feature docs, tests, and user-visible APIs.
- Docs clearly distinguish stable, experimental, planned, archived, and out-of-scope behavior.
