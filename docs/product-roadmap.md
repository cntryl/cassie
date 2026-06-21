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
| Projection diffing and multi-instance consistency checks | Planned | Cassie-specific |

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
| Adaptive feedback and cost-informed planning | Implemented/Planned by depth | Experimental |

## Search & AI

Goal: expose document-native search, vector, hybrid, and embedding workflows through Cassie SQL and APIs.

| Feature Area | Status | Compatibility |
| --- | --- | --- |
| Full-text inverted index and BM25 | Implemented | Cassie-specific |
| `search`, `search_score`, `snippet` | Implemented | Cassie-specific |
| Vector values and distance functions | Implemented | Cassie-specific with pgvector-style operators |
| HNSW vector indexes | Implemented | Experimental |
| IVFFlat vector indexes | Planned/Experimental | Experimental |
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
| Time-series indexes | Planned | Experimental |
| EXPLAIN, EXPLAIN ANALYZE, metrics | Implemented | Experimental output format |

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
- Build phase 05 around write-side performance contracts, replay/ingest batching, locality, and write-amplification control.
- Build phase 06 around read-side performance contracts, Midge-native access paths, access-path assertions, and projection-shaped reads.
- Tighten PostgreSQL compatibility documentation for already-implemented SQL features through the read-model access lens.
- Expand client compatibility probes for psql, sqlx, diesel, prisma, and SQLAlchemy read-model workflows.
- Promote experimental catalog, procedure, rollup, HNSW, and embedding surfaces as their compatibility guarantees settle.
- Add performance evidence for production-ready claims on planner, index, search, vector, and analytics paths.
- Continue splitting large legacy modules before adding broad feature work in those areas.
