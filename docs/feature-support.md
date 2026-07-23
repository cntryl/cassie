# Feature Support

This is the sole canonical behavior and status matrix for Cassie. Each capability has exactly one status:

- **Stable**: implemented, tested, documented, and supported within the current pre-release baseline.
- **Experimental**: implemented for documented cases, but correctness, resource bounds, evidence, or compatibility still has an open closure item.
- **Planned**: accepted future work that is not implemented as a supported contract.

`Production-ready` is not a feature status. It is an evidence classification owned by [Production Readiness](production-readiness.md).

The current beta support envelope consists of capabilities marked Stable. Experimental capabilities ship for evaluation with explicit limits and are not compatibility commitments. See [Production Readiness](production-readiness.md) for the release evidence bar.

## Ownership Boundary

Midge owns persistence, durability, and recovery mechanics. Cassie owns logical query layouts and query-visible failures, including SQL semantics, indexes, planning, execution, caching, cancellation, memory and result limits, and protocol error mapping. Cassie does not implement a parallel WAL, recovery engine, or storage abstraction.

Cassie is permanently a single-node query engine. Distributed SQL, cluster membership or management, replication, consensus, sharding and rebalancing, cross-node transactions, multi-node planning, remote query forwarding, and automatic cross-node repair are product non-goals. External systems may route to independent nodes, but that does not expand Cassie's execution or coordination boundary.

The only accepted Cassie-owned on-disk baseline marker is `cassie-midge-layout-v1`. A directory without that marker is rejected with a recreate diagnostic. There is no migration or legacy reader.

## Relational SQL

| Capability | Behavior | Status |
| --- | --- | --- |
| Core reads | `SELECT`, projection, aliases, expressions, `FROM`, `WHERE` | Stable |
| Predicates and nulls | Comparison, boolean logic, `IS NULL`, `IN`, `BETWEEN`, three-valued logic | Stable |
| Ordering and pagination | `ORDER BY`, null placement, `LIMIT`, `OFFSET` | Stable |
| Deduplication | `DISTINCT`, `DISTINCT ON` | Stable |
| Aggregation | `count`, `sum`, `avg`, `min`, `max`, grouping and `HAVING` | Stable |
| Join syntax | Inner, left, right, full outer, cross, lateral, apply, semi, and anti forms with legality-preserving planning | Experimental |
| Subqueries | Scalar, table, predicate, lateral, and correlated forms | Experimental |
| Common table expressions | Non-recursive and recursive `WITH` | Experimental |
| Set operations | `UNION`, `UNION ALL`, `INTERSECT`, `EXCEPT` | Stable |
| Window functions | Ranking, offset, value functions, and documented row frames | Experimental |
| Views | Read-only views and nested views | Stable |
| User functions | Scalar UDFs with declared volatility | Experimental |
| Procedures | Limited compatibility and administration surface | Experimental |
| Types and casts | Text, numeric, bool, timestamp, UUID, JSON, arrays, vectors, and supported casts | Experimental |

## Mutation and Catalog

| Capability | Behavior | Status |
| --- | --- | --- |
| DML | `INSERT`, PostgreSQL-style `ON CONFLICT`, `UPDATE`, `DELETE`, `RETURNING`, CSV copy ingestion | Experimental |
| Transactions | Begin, commit, rollback, savepoints, read-your-writes | Experimental |
| Database and schema scope | Databases, schemas, persisted `search_path`, qualified names, and administrator-managed `CONNECT` grants with live-session revalidation | Stable |
| Tables and constraints | Table DDL, name-idempotent `CREATE TABLE IF NOT EXISTS`, defaults, unique, check, foreign key | Experimental |
| Scalar indexes | Primary, secondary, composite, unique, covering, partial, expression, and name-idempotent `CREATE INDEX IF NOT EXISTS` | Experimental |
| Virtual catalogs | PostgreSQL-like and Cassie runtime catalog views | Experimental |
| Projection lifecycle | Checkpoints, replay, materialization, refresh, version activation | Experimental |
| Verification and repair | Hashes, manifests, local repair planning and audit | Experimental |

## Retrieval and Analytics

| Capability | Behavior | Status |
| --- | --- | --- |
| Full-text search | Persisted posting-block reads, exact BM25 scoring, snippets, bounded candidate fetches, transaction overlays, and labelled artifact fallback | Stable |
| Exact vector search | Cosine, dot, and L2 scoring with streaming bounded top-k, cancellation, and hard memory limits | Stable |
| HNSW | Persisted graph point-read candidates, deterministic bounded expansion, and exact source-row reranking | Stable |
| IVFFlat | Persisted centroid membership-prefix candidates, deterministic probes, and exact source-row reranking | Stable |
| Hybrid retrieval | Persisted text, vector, and structured candidate intersection before exact final scoring | Stable |
| Embedding providers | Controlled and response-bounded OpenAI, OpenAI-compatible, TEI, Ollama, Voyage, Cohere, and deterministic local protocols; third-party availability is not guaranteed | Stable |
| Time-series access | Ordered partition/range lookup, bucketing, rollups, retention | Experimental |
| Column-batch analytics | Typed batches, pruning, accelerated aggregates | Experimental |
| Graph traversal | Neighbor expansion and shortest paths | Experimental |

## Planning, Execution, and Interfaces

| Capability | Behavior | Status |
| --- | --- | --- |
| Plan cache | Session-safe reusable parsed and physical plans | Experimental |
| Execution-result cache | Context-isolated, epoch-invalidated, byte-bounded query results | Experimental |
| Cost planning | Relational join enumeration, deterministic statistics fallbacks, physical properties, and access-path selection | Experimental |
| Adaptive planning | Feedback-informed selection and checkpointed operator switching | Experimental |
| Pull execution | Bounded batch streams and early termination | Experimental |
| Query controls | Deadline, cancellation, SQL complexity, transport write, result, candidate, worker, and memory bounds | Experimental |
| Configurable parallelism | Shared worker permits with deterministic merges | Experimental |
| Pgwire | Primary SQL interface; detailed contract in compatibility documentation | Experimental |
| REST | Secondary administrative and resource API | Experimental |
| Admin UI | Local operational interface over REST | Experimental |

## Intentional Limits

Cassie does not claim full PostgreSQL syntax or catalog parity, distributed execution or cluster management, trigger-based application logic, or general OLTP behavior. Unsupported syntax and resource exhaustion return deterministic query-visible errors instead of silently changing semantics.
