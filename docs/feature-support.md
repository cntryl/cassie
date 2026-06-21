# Feature Support

This matrix records Cassie's supported feature surface as an event-sourced read-model database. It covers implemented behavior as well as experimental and planned areas that need compatibility or production-readiness work.

Scope is determined by read-model usefulness, not database taxonomy. Relational, analytical, search, vector, and time-series capabilities belong here when they are needed to serve real projection workloads.

Status terms:

- `Stable`: implemented, tested, documented, and intended to remain compatible within the same major line.
- `Experimental`: implemented or partially implemented, but compatibility or output shape may still change.
- `Planned`: accepted roadmap area without a production guarantee.
- `Cassie-specific`: intentionally exposes Cassie behavior rather than PostgreSQL parity.

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
| DML | INSERT, UPDATE, DELETE, RETURNING | Stable | PostgreSQL-like |
| DDL | CREATE TABLE, ALTER TABLE, DROP TABLE, CREATE SCHEMA, DROP SCHEMA, CREATE INDEX, DROP INDEX, CREATE ROLLUP, REFRESH ROLLUP, DROP ROLLUP, CREATE/ALTER/DROP/ENFORCE RETENTION POLICY | Stable/Experimental by object type | PostgreSQL-like plus Cassie-specific analytics |
| Transactions | BEGIN, COMMIT, ROLLBACK, savepoints | Stable | PostgreSQL-like with Cassie/Midge durability notes |
| Views | CREATE VIEW, DROP VIEW, nested views | Stable | PostgreSQL-like read-only view behavior |
| Functions | scalar functions, user-defined functions | Stable/Experimental | PostgreSQL-like where documented |
| Procedures | CREATE PROCEDURE, CALL | Experimental | PostgreSQL-like syntax, Cassie execution semantics |
| Types | text, bool, integers, floats, decimal, timestamp, uuid, json, arrays, vector | Stable/Experimental | PostgreSQL-like plus Cassie vector |
| Casts | CAST(x AS type), x::type | Stable | PostgreSQL-like |

## Cassie-Specific Query Features

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| Full-text | search(field, query), search_score(field, query), snippet(field, query) | Stable | Cassie-specific |
| Vector | vector_score, vector_distance, cosine_distance, dot_product, l2_distance | Stable | Cassie-specific |
| pgvector syntax | `<=>`, `<->`, `<#>`, vector(n) | Stable/Experimental | pgvector-style, not full extension parity |
| Hybrid | hybrid_score(text_score, vector_score) | Stable | Cassie-specific |
| Embeddings | provider, model, dimensions, metric validation | Experimental | Cassie-specific |
| Projections | projection metadata, source checkpoints, freshness, replay batch diagnostics, schema version, offset, lag, rebuild state | Experimental | Cassie-specific |
| Projection lifecycle | internal idempotent replay ingestion, materialized projections, analytical projection options, versioned builds, verification-aware active-version swaps, operations views | Experimental | Cassie-specific |
| Time series | time_bucket fixed windows, exact-match materialized rollups over deterministic aggregates, explicit retention policies, range queries | Experimental | Cassie-specific deterministic semantics |
| Verification | deterministic row hashes, range hashes, projection roots, rebuild verification metadata, `VERIFY PROJECTION`, `DIFF PROJECTION`, `COMPARE PROJECTION`, local integrity reports | Experimental | Cassie-specific |

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
| Joins | nested-loop, hash join, merge join, semi join, anti join | Stable/Experimental | Semantics-preserving |
| Aggregation | hash aggregate, sort aggregate | Stable | Semantics-preserving |
| Distinct | hash distinct, sort distinct | Stable | Semantics-preserving |
| Execution | row executor, batch/vectorized executor | Stable/Experimental | Internal |
| Caching | plan cache, function cache, prepared statement cache | Stable/Experimental | Internal |
| Adaptive | runtime stats, cardinality feedback, adaptive candidate sizing | Experimental | Internal |
| Parallel | parallel scan, parallel scoring, parallel aggregation | Experimental | Semantics-preserving |

## Protocol and API Support

| Category | Supported Items | Status | Compatibility |
| --- | --- | --- | --- |
| PostgreSQL wire | startup, auth, simple query, extended query, parse, bind, describe, execute, sync, close | Stable | PostgreSQL-compatible subset |
| Pgwire results | row description, data row, command complete, error response, ready for query | Stable | PostgreSQL-compatible subset |
| Pgwire compatibility | prepared statements, portals, text/binary formats, catalog introspection | Stable/Experimental | PostgreSQL-compatible subset |
| HTTP | SQL query, search query, vector query, hybrid query, document APIs, admin APIs | Stable/Experimental | Cassie REST API |
| Observability | EXPLAIN, EXPLAIN ANALYZE, query stats, operator stats, cost-model diagnostics, index used, index feedback marker, time-series index diagnostics, column-batch index used, aggregate acceleration, rollup rewrite selected, mixed execution stages, analytical projection markers, rows scanned | Experimental | PostgreSQL-like entry points with Cassie output |
| Projection operations | active version, source checkpoint, lag, freshness, rebuild state, verification state, root state, last replay batch, last error, version state | Experimental | Cassie-specific |
| Metrics | latency, throughput, errors, cache hit rate, projection replay/build/swap/stale/hash/verification/integrity/mixed-fallback counters, retention enforcement/delete/skip counters, rollup refresh/rewrite/fallback counters, column-batch scan/fallback/byte/segment/column counters, aggregate acceleration counters | Experimental | Cassie-specific |

## Projection Verification Surfaces

- `VERIFY PROJECTION <name> [VERSION <version_id>] [MODE metadata_only|hashes_only|indexes_only|full]` runs a read-only local integrity check and persists the latest report.
- `DIFF PROJECTION <left> [VERSION <version_id>] WITH <right> [VERSION <version_id>] [LIMIT n] [AFTER cursor]` returns deterministic local hash differences or an explicit unverifiable result.
- `COMPARE PROJECTION <name> [VERSION <version_id>] WITH MANIFEST '<json>'` compares the current local root digest with an imported manifest digest.
- `pg_catalog.pg_projection_hashes` exposes row/range/root hash state, algorithm metadata, coverage counts, and root digest.
- `pg_catalog.pg_projection_operations` exposes freshness, rebuild, active-version, verification, and root state.
- `pg_catalog.pg_projection_integrity_reports` exposes the latest local integrity report.
- `pg_catalog.pg_projection_comparison_reports` exposes persisted local-vs-manifest comparison reports after restart hydration.
- EXPLAIN includes `cost_model`, `selected_cost`, `rejected_alternatives`, `mixed_execution`, `mixed_stages`, `exact_baseline`, `analytical_projection`, and `projection_freshness` diagnostics for mixed search/vector/analytical plans.

## Compatibility Notes

- See [PostgreSQL Compatibility](postgres-compatibility.md) for supported, unsupported, and intentionally different PostgreSQL behavior.
- See [Product Roadmap](product-roadmap.md) for milestone grouping and remaining roadmap themes.
- See [Definition of Done](definition-of-done.md) for how a feature moves from implemented to stable or production-ready.
