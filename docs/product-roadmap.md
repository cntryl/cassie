# Product Roadmap

Cassie's roadmap is organized around its role as a high-performance read-model database for event-sourced systems. PostgreSQL-compatible SQL and pgwire access exist to make projections easy to query from familiar tools; they are not a commitment to full OLTP PostgreSQL parity.

Implemented areas remain on the roadmap until their compatibility notes, ownership, definition of done, and production-readiness guarantees are explicit.

Feature priority is determined by read-model need. If users need a capability to build, operate, analyze, search, report on, or serve projections, it is in scope regardless of whether the capability resembles OLTP, OLAP, search, vector retrieval, or time-series database work.

## Status Model

| Status | Meaning |
| --- | --- |
| Implemented | Code and tests exist for the feature area. Documentation and compatibility notes may still need refinement. |
| Experimental | Feature works for supported cases, but behavior or compatibility may still change. |
| Planned | Feature area is accepted on the roadmap but not fully implemented. |
| Production-ready | Implemented, tested, documented, benchmarked where performance-sensitive, and compatibility boundaries are explicit. |

See [Production Readiness](production-readiness.md) for feature-family readiness, evidence, operational signals, restart coverage, and blockers. See [Experimental Promotion Criteria](experimental-promotion-criteria.md) for the evidence gates required before a future issue promotes or narrows an experimental surface. Roadmap implementation status does not by itself promote a feature to production-ready.

## Near-Term Backlog

The current engineering backlog is cleanup-first. New feature work should not start while these items remain open unless the work is required to complete one of the cleanup items below.

| Priority | Backlog Item | Status | Why it blocks feature work |
| --- | --- | --- | --- |
| P0 | Drive repo-wide `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` to zero | Implemented baseline | Repo-wide pedantic validation is part of the required completion loop; new findings are treated as defects before feature work continues. |
| P0 | Restore deterministic repo-wide test execution so `cargo test --locked` completes or fails without hangs | Implemented baseline | Full-suite validation is expected to complete; new hangs are treated as blocking regressions. |
| P0 | Repair pgwire compatibility drift between documented extended-query support and the current implementation | Implemented | Extended-query parse, bind, describe, execute, close, sync, flush, prepared-statement, and portal lifecycle paths now have pgwire and `tokio-postgres` coverage. |
| P1 | Reduce architecture drift in oversized orchestration modules and compatibility shims before adding behavior | In progress | Large cross-cutting files and drifted boundaries increase refactor risk, hide ownership, and make pedantic cleanup more expensive. |
| P1 | Reconcile docs, tests, and implementation whenever compatibility claims are narrower than the code or broader than actual behavior | In progress | Cleanup must leave explicit contracts, not stale claims that silently regress under new work. |

## Projection Lifecycle & Replay Safety

Goal: make projection construction, replay, rebuilds, freshness, and activation deterministic and observable.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Projection metadata, schema version, offset, lag, rebuild state | Implemented | Experimental Cassie-specific |
| Projection source checkpoints and replay metadata | Implemented | Experimental Cassie-specific |
| Idempotent replay ingestion | Implemented | Experimental Cassie-specific internal API |
| Projection handler determinism and replay failure contracts | Implemented | Experimental Cassie-specific internal API |
| Materialized projections | Implemented | Experimental Cassie-specific |
| Projection versioning | Implemented | Experimental Cassie-specific |
| Projection active-version swaps | Implemented | Experimental Cassie-specific |
| Projection operations catalog views and metrics | Implemented | Experimental Cassie-specific |

## Verification & Integrity

Goal: prove rebuilt read models and derived state are internally consistent before they are trusted operationally.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Row hashes | Implemented | Experimental Cassie-specific |
| Range hashes | Implemented | Experimental Cassie-specific |
| Projection Merkle roots | Implemented | Experimental Cassie-specific |
| Rebuild verification | Implemented | Experimental Cassie-specific |
| Integrity verification | Implemented | Experimental Cassie-specific |
| Projection diffing and manifest comparison | Implemented baseline | Experimental Cassie-specific |
| Projection repair planning and local repair audit | Implemented baseline | Experimental Cassie-specific admin workflow |
| Multi-instance consistency checks | Implemented baseline | Cassie-specific |

## SQL Foundation

Goal: provide the core PostgreSQL-like query surface expected by application code, reporting tools, and ORMs that query read-model projections.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Core SELECT, projection, ordering, pagination, DISTINCT | Implemented | Stable |
| Predicates, nulls, lists, ranges | Implemented | Stable |
| Aggregates, GROUP BY, HAVING | Implemented | Stable |
| Joins, EXISTS, NOT EXISTS, lateral forms | Implemented | Stable |
| Subqueries and correlated subqueries | Implemented | Stable |
| CTEs and recursive CTEs | Implemented | Stable |
| Set operations | Implemented | Stable |
| Window functions | Implemented | Stable with documented frame limits |
| DML and RETURNING | Implemented | Stable for projection-state mutation paths |
| Transactions and savepoints | Implemented | Stable with Cassie/Midge durability notes for projection workflows |

## Schema & Catalog

Goal: make schema definition, metadata, and introspection predictable for users and PostgreSQL-compatible clients.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Tables and schemas | Implemented | Stable |
| Constraints and defaults | Implemented | Stable |
| Views and nested views | Implemented | Stable |
| Limited procedures and CALL | Implemented | Experimental compatibility/admin surface |
| Catalog metadata and virtual views | Implemented | Experimental |
| Client catalog probes | Implemented | Experimental |

## Indexing & Optimization

Goal: provide predictable index behavior and visible planner decisions without adding a second storage abstraction.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Primary and scalar secondary indexes | Implemented | Stable |
| Composite indexes | Implemented | Stable |
| Unique indexes and constraints | Implemented | Stable |
| Covering indexes | Implemented | Stable |
| Partial indexes | Implemented | Experimental predicate implication |
| Expression indexes | Implemented | Experimental expression equivalence |
| Planner optimization | Implemented | Stable result semantics |
| Adaptive feedback and cost-informed planning | Implemented guarded baseline/Planned by depth | Experimental |

## Search & AI

Goal: expose document-native search, vector, hybrid, and embedding workflows through Cassie SQL and APIs.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Full-text inverted index and BM25 | Implemented | Cassie-specific |
| `search`, `search_score`, `snippet` | Implemented | Cassie-specific |
| Vector values and distance functions | Implemented | Cassie-specific with pgvector-style operators |
| HNSW vector indexes | Implemented | Experimental |
| IVFFlat vector index metadata/options | Implemented | Experimental |
| IVFFlat trained candidate execution | Implemented | Experimental exact re-rank |
| Hybrid scoring | Implemented | Cassie-specific |
| Graph retrieval table functions | Implemented baseline | Experimental Cassie-specific |
| Embedding providers and validation | Implemented | Experimental |

## Analytics

Goal: provide analytical read acceleration and operational visibility while keeping row blobs as the source of truth.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Column batches and covered scans | Implemented | Cassie-specific |
| Segment pruning | Implemented | Cassie-specific |
| Aggregate acceleration | Implemented | Cassie-specific |
| `time_bucket` fixed windows | Implemented | Cassie-specific deterministic function |
| Rollups | Implemented | Experimental |
| Retention policies | Implemented | Experimental explicit enforcement |
| Time-series index metadata and range planning | Implemented baseline | Experimental |
| Time-series row-backed range scans and bucket diagnostics | Implemented baseline | Experimental |
| Persisted bucket-native time-series index storage | Implemented baseline | Experimental |
| Analytical projection options and covered-query routing | Implemented | Experimental Cassie-specific |
| EXPLAIN, EXPLAIN ANALYZE, metrics | Implemented | Experimental output format |

## Foundation Contracts

Goal: define the runtime and access-path contracts that later write/read optimization must preserve.

Phase 04 is complete and archived in `docs/performance-contracts.md` and `issues/phase-04/README.md`.
Phase 04 treats pgwire and REST as async interfaces over a synchronous Rust engine.
Supported runtime paths must define where async IO stops, where synchronous engine work starts, and which blocking boundary protects Tokio worker tasks.
Phase 04 also defines read access-path vocabulary before write-side index/key-layout work or read-side planner/executor work consumes it.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Runtime-boundary contracts | Implemented | Cassie-specific internal/runtime contract |
| Pgwire blocking boundaries | Implemented | Internal runtime behavior, stable protocol semantics |
| REST blocking boundaries | Implemented | Internal runtime behavior, stable HTTP semantics |
| Auth and embedding blocking discipline | Implemented | Internal runtime behavior |
| Runtime-boundary diagnostics | Implemented | Experimental metrics/admin diagnostics |
| Boundary regression tests and static audit | Implemented | Internal test discipline |
| Read access-path contracts | Implemented | Cassie-specific internal/perf contract |

## Write Optimization

Goal: compile write-side read-model workflows into Midge-efficient write paths without weakening deterministic replay, freshness, verification, or projection lifecycle semantics.

Phase 05 write optimization is implemented and documented; the archived contract and diagnostics surface lives in `docs/performance-contracts.md`.

Phase 05 treats SQL, REST, replay, and rebuild commands as write interfaces, not as a requirement to use the same per-row mutation path for every workload.
Supported write patterns must define required and forbidden write-path behavior, write amplification budgets, and benchmark or diagnostic evidence.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Write-path performance contracts | Implemented | Cassie-specific internal/perf contract |
| Replay and ingest batching | Implemented | Cassie-specific internal |
| Duplicate replay skip without row/index rewrites | Implemented | Cassie-specific internal |
| Index maintenance batching and delta coalescing | Implemented | Cassie-specific internal |
| Write-locality key/layout optimization | Implemented | Cassie-specific internal |
| Bulk rebuild/ingest fast paths | Implemented | Cassie-specific internal |
| Write amplification diagnostics and budgets | Implemented | Experimental metrics/admin diagnostics |

## Read Optimization

Goal: compile supported read-model query patterns into Midge-native access paths and make those access paths explicit, testable, and benchmarked.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Read-path performance contracts | Implemented baseline | Experimental documentation plus benchmark feedback |
| 10k/100k manual benchmark scenarios | Implemented baseline | Criterion-backed developer feedback loop |
| Access-path assertions and EXPLAIN guarantees | Implemented baseline/Planned by depth | Experimental |
| Predicate/order/limit pushdown to storage-native scans | Implemented baseline/Planned by depth | Experimental |
| Keyset pagination and bounded continuation scans | Implemented baseline | Experimental |
| Top-K, early-stop, and bounded candidate execution | Implemented baseline | Experimental |
| Projection-shaped read layouts for latency-sensitive patterns | Documented baseline/Planned by depth | Cassie-specific |

## Postgres Compatibility

Goal: support practical PostgreSQL client interoperability for read-model access without claiming full PostgreSQL server equivalence.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Pgwire startup and authentication | Implemented | Stable |
| Simple query protocol | Implemented | Stable |
| Extended query protocol | Implemented | Stable |
| Prepared statements and portals | Implemented | Stable |
| SQLSTATE-style error responses | Implemented | Experimental mapping completeness |
| Catalog probes | Implemented | Experimental |
| psql, sqlx, diesel, prisma, SQLAlchemy matrix | Implemented baseline | Experimental |

## Operational Scale

Goal: support horizontal expansion through externally orchestrated independent Cassie read nodes without adding distributed SQL semantics.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Local node/projection/tenant/partition assignment metadata | Implemented baseline | Experimental Cassie-specific |
| Assignment catalog diagnostics | Implemented baseline | Experimental Cassie-specific |
| External routing and projection ownership orchestration | Documented contract | External system contract |
| Snapshot and restore | Implemented baseline | Experimental Cassie-specific local recovery |
| Capacity management guidance | Documented baseline | Experimental operational documentation |

## Remaining Roadmap Themes

- Keep projection repair admin-only, audited, local, verification-led, and aligned with the [Projection Repair Runbook](projection-repair-runbook.md) as unsupported repair scopes mature.
- Keep operational scale local and externally orchestrated: Cassie exposes assignment metadata and a router/drain/move contract, but does not perform distributed query planning, cross-node routing, replication, quorum reads, or consensus.
- Use [Capacity Management](capacity-management.md) as the current advisory sizing baseline; `/metrics.capacity` reports local logical key/value bytes, while automatic admission control and capacity movement remain future depth.
- Improve manual performance scenarios as benchmark evidence stabilizes and larger fixtures become practical.
- Prioritize query patterns required by real read models over feature parity with any general-purpose database.
- Treat the archived phase 04 contract surface as the reference for explicit async transport boundaries, synchronous engine paths, blocking offload, runtime-boundary diagnostics, and read access-path contracts.
- Keep future write-path changes aligned with the archived phase 05 contracts in `docs/performance-contracts.md`.
- Treat the archived phase 06 surface in `issues/phase-06/README.md` plus the Phase 09 read-path depth in `docs/performance-contracts.md` as the reference for implemented Midge-native read paths, access-path assertions, and projection-shaped read diagnostics. Remaining read-optimization depth is limited to explicit follow-on slices such as broader mixed-direction suffix ordering and adaptive side selection for late-match bounded joins.
- Treat the archived phase 07 surface in `issues/phase-07/README.md` as the reference for advanced query, adaptive execution, column-store table mode, and offline consistency-comparison behavior.
- Treat `issues/phase-08/README.md` as the archived README-goal closure surface for operational metadata, snapshot/restore, repair, read optimization, time-series, client compatibility, production classification, and capacity-management documentation.
- Use `issues/phase-09/README.md` as the archived production-depth follow-up surface; experimental promotion now follows [Experimental Promotion Criteria](experimental-promotion-criteria.md).
- Tighten PostgreSQL compatibility documentation for already-implemented SQL features through the read-model access lens.
- Expand remaining client compatibility probes for sqlx, diesel, prisma, broader SQLAlchemy reflection, and migration-tool read-model workflows.
- Promote experimental catalog, limited procedure, rollup, HNSW, and embedding surfaces only through surface-specific future issues that satisfy [Experimental Promotion Criteria](experimental-promotion-criteria.md).
- Add performance evidence for production-ready claims on planner, index, search, vector, and analytics paths.
- Continue splitting large legacy modules before adding broad feature work in those areas.
