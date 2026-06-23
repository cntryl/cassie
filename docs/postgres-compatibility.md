# PostgreSQL Compatibility

Cassie uses PostgreSQL wire protocol as the primary query interface and intentionally supports a PostgreSQL-like SQL surface. Compatibility means practical client interoperability for read-model projections, not full PostgreSQL server equivalence.

Cassie does not seek PostgreSQL feature parity for its own sake. PostgreSQL-like behavior is prioritized when it helps users build, query, inspect, analyze, report on, or operate read models through existing tools and applications.

## Compatibility Guarantees

| Level | Guarantee |
| --- | --- |
| Stable | Supported SQL/protocol behavior is expected to remain compatible within the same major line. Regressions should be treated as bugs. |
| Experimental | Behavior is implemented and tested for known cases, but exact compatibility or output shape may change. |
| Unsupported | The feature is not available and should return a deterministic error when reachable. |
| Cassie-specific | The feature intentionally exposes Cassie behavior and has no PostgreSQL-equivalent guarantee. |

## Supported PostgreSQL-Like SQL

The supported SQL surface exists so applications, ORMs, reporting tools, and operators can query and maintain projection state through familiar workflows.

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
- CREATE TABLE, ALTER TABLE, DROP TABLE, CREATE SCHEMA, DROP SCHEMA, CREATE INDEX, DROP INDEX, CREATE VIEW, and DROP VIEW.
- Limited experimental CREATE PROCEDURE and CALL support for compatibility/admin workflows.
- CAST(x AS type) and PostgreSQL-style x::type casts.

Cassie-specific read-model commands:

- CREATE MATERIALIZED PROJECTION, REFRESH MATERIALIZED PROJECTION, DROP MATERIALIZED PROJECTION.
- ALTER MATERIALIZED PROJECTION BUILD VERSION.
- ALTER MATERIALIZED PROJECTION ACTIVATE VERSION.
- DROP MATERIALIZED PROJECTION VERSION.

Unsupported or not yet guaranteed:

- Full PostgreSQL grammar parity.
- PostgreSQL table inheritance, partitions, storage parameters, operator classes, collations, deferrable constraints, security-barrier views, updatable views, and procedural language parity.
- Stored-procedure business-logic platforms, triggers, PL/pgSQL, dynamic SQL, exception blocks, procedure-local transaction control, recursive procedure workflows, and trigger-driven application behavior.
- Full PostgreSQL system catalog parity.
- PostgreSQL optimizer hint behavior or EXPLAIN output parity.

Intentional differences:

- Cassie stores tables as Midge-backed collections and row blobs.
- Cassie materialized projections are read-only projection outputs with Cassie-specific lifecycle and versioning commands.
- Cassie treats DML and transactions as projection-state mutation and operational correction tools, not a general OLTP workload contract.
- Cassie procedure support executes supported Cassie SQL with Cassie semantics; it is not PostgreSQL procedural-language compatibility.
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

## Client Compatibility Matrix

The matrix tracks read-model workflows, not full PostgreSQL server equivalence. A client is supported only for the specific connection, query, catalog, projection, and error-handling behavior listed here.

| Client/workflow | Status | Validated read-model workflows | Validation |
| --- | --- | --- | --- |
| `tokio-postgres` | Supported baseline | Startup without password, simple query, extended prepared query, DDL/DML round trip, `ON CONFLICT`, foreign-key errors, NOT NULL/unique SQLSTATE metadata, recursive CTEs, syntax-error recovery, selected catalog metadata | Default `cargo test --locked --test compatibility_matrix` |
| `psql` | Experimental opt-in | Non-interactive connection, simple DDL/DML, simple SELECT output, and operational smoke usage against pgwire | Ignored `should_validate_psql_read_model_probe_when_enabled`; run `CASSIE_RUN_PSQL_COMPAT=1 cargo test --locked --test compatibility_matrix should_validate_psql_read_model_probe_when_enabled -- --ignored --nocapture` with local `psql` installed. Set `CASSIE_PSQL_BIN` to override the binary. |
| `sqlx` | Untested/planned | Prepared read queries, connection pooling, compile-time or offline query checks for supported SQL, catalog probes used by migrations | No automated probe yet |
| `diesel` | Untested/planned | Projection-table reads and supported schema metadata where Diesel does not require unsupported PostgreSQL catalog parity | No automated probe yet |
| `prisma` | Untested/planned | Introspection and read queries for compatible projection tables where Prisma does not require unsupported catalog/DDL features | No automated probe yet |
| `SQLAlchemy` | Untested/planned | Reflection and read queries over supported tables/views; SQLAlchemy Core query execution where generated SQL stays inside Cassie's supported surface | No automated probe yet |
| Common migration tools | Experimental/documented | Supported DDL through pgwire: schemas, tables, constraints, indexes, and views that map to Cassie SQL | Use tool-specific dry runs against a disposable Cassie node; advanced PostgreSQL migration features remain unsupported unless documented separately |

Unsupported or out-of-scope for client compatibility:

- PostgreSQL server parity checks that require complete `pg_catalog` or `information_schema` behavior.
- Client workflows requiring extensions, triggers, PL/pgSQL, stored-procedure business logic, LISTEN/NOTIFY, COPY, table inheritance, partitions, logical replication, advisory locks, two-phase commit, or PostgreSQL storage parameters.
- ORM-generated OLTP workloads that depend on distributed transactions, row-locking semantics, trigger business logic, or PostgreSQL optimizer behavior.
- Migration diffs that assume PostgreSQL-owned physical storage, operator classes, collations, or extension-managed metadata.

Future client probes should stay isolated from the default suite unless the dependency is lightweight, deterministic, and available without external services.

## Cassie-Specific SQL and APIs

These features are intentionally Cassie-specific:

- Projection source checkpoints, replay metadata, freshness, versioning, swaps, and verification diagnostics.
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
