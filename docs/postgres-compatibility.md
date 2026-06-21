# PostgreSQL Compatibility

Cassie uses PostgreSQL wire protocol as the primary query interface and intentionally supports a PostgreSQL-like SQL surface. Compatibility means practical client interoperability for supported behavior, not full PostgreSQL server equivalence.

## Compatibility Guarantees

| Level | Guarantee |
| --- | --- |
| Stable | Supported SQL/protocol behavior is expected to remain compatible within the same major line. Regressions should be treated as bugs. |
| Experimental | Behavior is implemented and tested for known cases, but exact compatibility or output shape may change. |
| Unsupported | The feature is not available and should return a deterministic error when reachable. |
| Cassie-specific | The feature intentionally exposes Cassie behavior and has no PostgreSQL-equivalent guarantee. |

## Supported PostgreSQL-Like SQL

Supported:

- SELECT, FROM, WHERE, projection aliases, expressions, ORDER BY, LIMIT, OFFSET, DISTINCT, and DISTINCT ON.
- Comparison predicates, boolean predicates, null checks, IN, NOT IN, BETWEEN, and NOT BETWEEN.
- count, sum, avg, min, max, GROUP BY, and HAVING.
- INNER, LEFT, RIGHT, FULL OUTER, CROSS, semi, anti, and lateral-style joins.
- Scalar subqueries, FROM subqueries, predicate subqueries, and correlated subqueries.
- WITH and WITH RECURSIVE.
- UNION, UNION ALL, INTERSECT, and EXCEPT.
- row_number, rank, dense_rank, lag, lead, first_value, and last_value.
- INSERT, UPDATE, DELETE, and RETURNING.
- BEGIN, COMMIT, ROLLBACK, SAVEPOINT, ROLLBACK TO, and RELEASE SAVEPOINT.
- CREATE TABLE, ALTER TABLE, DROP TABLE, CREATE SCHEMA, DROP SCHEMA, CREATE INDEX, DROP INDEX, CREATE VIEW, DROP VIEW, CREATE PROCEDURE, and CALL.
- CAST(x AS type) and PostgreSQL-style x::type casts.

Unsupported or not yet guaranteed:

- Full PostgreSQL grammar parity.
- PostgreSQL table inheritance, partitions, storage parameters, operator classes, collations, deferrable constraints, security-barrier views, updatable views, and procedural language parity.
- Full PostgreSQL system catalog parity.
- PostgreSQL optimizer hint behavior or EXPLAIN output parity.

Intentional differences:

- Cassie stores tables as Midge-backed collections and row blobs.
- Cassie planner and executor may choose index, full-text, vector, hybrid, column-batch, or aggregate-acceleration paths that PostgreSQL does not have.
- Some catalog rows are compatibility shims over Cassie metadata.

## Wire Protocol

Supported:

- Startup and authentication.
- Simple query flow.
- Extended query flow: parse, bind, describe, execute, sync, and close.
- Prepared statements and portals.
- Row description, data row, command complete, error response, and ready-for-query messages.
- Text and binary format paths covered by tests.

Unsupported or not yet guaranteed:

- Full PostgreSQL backend protocol parity.
- Every optional message type or server parameter exposed by PostgreSQL.
- Exhaustive SQLSTATE parity.

Intentional differences:

- Protocol behavior maps to Cassie execution, catalog, and error surfaces.
- Unsupported features should produce deterministic PostgreSQL-style errors where possible.

## Catalog Compatibility

Supported:

- Catalog introspection required by current Cassie tests.
- Compatibility probes for supported table, schema, index, constraint, and view metadata.
- Virtual catalog views backed by Cassie metadata.

Unsupported or not yet guaranteed:

- Complete `pg_catalog` parity.
- Complete `information_schema` parity.
- All ORM-specific introspection probes.

The compatibility matrix should grow around real clients:

- psql
- sqlx
- diesel
- prisma
- SQLAlchemy
- common migration tools

## Cassie-Specific SQL and APIs

These features are intentionally Cassie-specific:

- `search(field, query)`, `search_score(field, query)`, and `snippet(field, query)`.
- `vector_score`, `vector_distance`, `cosine_distance`, `dot_product`, and `l2_distance`.
- pgvector-style operators implemented by Cassie, including `<=>`, `<->`, and `<#>`.
- `hybrid_score(text_score, vector_score)`.
- Embedding provider configuration and validation.
- Column-batch indexes and acceleration diagnostics.
- Rollup metadata and rewrite diagnostics.
- Cassie metrics and runtime feedback.

These may be exposed over PostgreSQL-compatible SQL or pgwire, but their semantics are Cassie-defined.

## Compatibility Work Required Before Closing a Feature

Every feature area should document:

- Supported PostgreSQL-compatible behavior.
- Unsupported PostgreSQL behavior.
- Intentional differences.
- SQLSTATE behavior for errors visible through pgwire.
- Client compatibility probes when relevant.
- Whether the feature is stable, experimental, planned, unsupported, or Cassie-specific.
