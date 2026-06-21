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

## Projection Lifecycle & Replay Safety

Goal: make projection construction, replay, rebuilds, freshness, and activation deterministic and observable.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Projection metadata, schema version, offset, lag, rebuild state | Implemented | Experimental Cassie-specific |
| Projection source checkpoints and replay metadata | Implemented | Experimental Cassie-specific |
| Idempotent replay ingestion | Implemented | Experimental Cassie-specific internal API |
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
| Multi-instance consistency checks | Planned | Cassie-specific |

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
| Procedures and CALL | Implemented | Experimental |
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
| Adaptive feedback and cost-informed planning | Implemented baseline/Planned by depth | Experimental |

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
| Time-series index storage maintenance and bucket scans | Planned | Experimental |
| Analytical projection options and covered-query routing | Implemented | Experimental Cassie-specific |
| EXPLAIN, EXPLAIN ANALYZE, metrics | Implemented | Experimental output format |

## Foundation Contracts

Goal: define the runtime and access-path contracts that later write/read optimization must preserve.

Phase 04 treats pgwire and REST as async interfaces over a synchronous Rust engine.
Supported runtime paths must define where async IO stops, where synchronous engine work starts, and which blocking boundary protects Tokio worker tasks.
Phase 04 also defines read access-path vocabulary before write-side index/key-layout work or read-side planner/executor work consumes it.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Runtime-boundary contracts | Planned | Cassie-specific internal/runtime contract |
| Pgwire blocking boundaries | Planned | Internal runtime behavior, stable protocol semantics |
| REST blocking boundaries | Planned | Internal runtime behavior, stable HTTP semantics |
| Auth and embedding blocking discipline | Planned | Internal runtime behavior |
| Runtime-boundary diagnostics | Planned | Experimental metrics/admin diagnostics |
| Boundary regression tests and static audit | Planned | Internal test discipline |
| Read access-path contracts | Planned | Cassie-specific internal/perf contract |

## Write Optimization

Goal: compile write-side read-model workflows into Midge-efficient write paths without weakening deterministic replay, freshness, verification, or projection lifecycle semantics.

Phase 05 treats SQL, REST, replay, and rebuild commands as write interfaces, not as a requirement to use the same per-row mutation path for every workload.
Supported write patterns must define required and forbidden write-path behavior, write amplification budgets, and benchmark or diagnostic evidence.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Write-path performance contracts | Planned | Cassie-specific internal/perf contract |
| Replay and ingest batching | Planned | Cassie-specific internal |
| Duplicate replay skip without row/index rewrites | Planned | Cassie-specific internal |
| Index maintenance batching and delta coalescing | Planned | Cassie-specific internal |
| Write-locality key/layout optimization | Planned | Cassie-specific internal |
| Bulk rebuild/ingest fast paths | Planned | Cassie-specific internal |
| Write amplification diagnostics and budgets | Planned | Experimental metrics/admin diagnostics |

## Read Optimization

Goal: compile supported read-model query patterns into Midge-native access paths and make those access paths explicit, testable, and benchmarked.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Read-path performance contracts | Implemented/Planned by depth | Experimental documentation plus benchmark enforcement |
| Access-path assertions and EXPLAIN guarantees | Planned | Experimental |
| Predicate/order/limit pushdown to storage-native scans | Planned | Experimental |
| Keyset pagination and bounded continuation scans | Planned | Experimental |
| Top-K, early-stop, and bounded candidate execution | Planned | Experimental |
| Projection-shaped read layouts for latency-sensitive patterns | Planned | Cassie-specific |

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
| psql, sqlx, diesel, prisma, SQLAlchemy matrix | Planned | Experimental |

## Remaining Roadmap Themes

- Harden verification gates and repair workflows beyond local read-only integrity reports.
- Promote performance targets for replay ingestion, projection rebuilds, verification, swaps, and lag catch-up from baseline benchmarks to measured thresholds.
- Prioritize query patterns required by real read models over feature parity with any general-purpose database.
- Build phase 04 around explicit async transport boundaries, synchronous engine paths, blocking offload, runtime-boundary diagnostics, and read access-path contracts.
- Build phase 05 around write-side performance contracts, replay/ingest batching, locality, and write-amplification control, using phase 04 issue 07 read-shape contracts before index/key-layout changes.
- Build phase 06 around Midge-native read implementation, access-path assertions, and projection-shaped reads using phase 04 issue 07 contracts.
- Keep phase 07 parked for advanced query and distributed backlog work until the required phase 04 through phase 06 gates are complete.
- Tighten PostgreSQL compatibility documentation for already-implemented SQL features through the read-model access lens.
- Expand client compatibility probes for psql, sqlx, diesel, prisma, and SQLAlchemy read-model workflows.
- Promote experimental catalog, procedure, rollup, HNSW, and embedding surfaces as their compatibility guarantees settle.
- Add performance evidence for production-ready claims on planner, index, search, vector, and analytics paths.
- Continue splitting large legacy modules before adding broad feature work in those areas.
