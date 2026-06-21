# Product Roadmap

Cassie's roadmap is organized by feature area rather than by missing work only. Implemented areas remain on the roadmap until their compatibility notes, ownership, definition of done, and production-readiness guarantees are explicit.

## Status Model

| Status | Meaning |
| --- | --- |
| Implemented | Code and tests exist for the feature area. Documentation and compatibility notes may still need refinement. |
| Experimental | Feature works for supported cases, but behavior or compatibility may still change. |
| Planned | Feature area is accepted on the roadmap but not fully implemented. |
| Production-ready | Implemented, tested, documented, benchmarked where performance-sensitive, and compatibility boundaries are explicit. |

## SQL Foundation

Goal: provide the core PostgreSQL-like query surface expected by application code and ORMs.

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
| DML and RETURNING | Implemented | Stable |
| Transactions and savepoints | Implemented | Stable with Cassie/Midge durability notes |

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
| Retention policies | Planned | Experimental |
| Time-series indexes | Planned | Experimental |
| EXPLAIN, EXPLAIN ANALYZE, metrics | Implemented | Experimental output format |

## Postgres Compatibility

Goal: support practical PostgreSQL client interoperability without claiming full PostgreSQL server equivalence.

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

- Tighten PostgreSQL compatibility documentation for already-implemented SQL features.
- Expand client compatibility probes for psql, sqlx, diesel, prisma, and SQLAlchemy.
- Promote experimental catalog, procedure, rollup, HNSW, and embedding surfaces as their compatibility guarantees settle.
- Add performance evidence for production-ready claims on planner, index, search, vector, and analytics paths.
- Continue splitting large legacy modules before adding broad feature work in those areas.
