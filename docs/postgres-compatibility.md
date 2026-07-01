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
- COPY FROM STDIN over pgwire simple query with CSV payloads and optional HEADER.
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
- COPY TO STDOUT, binary COPY, server-side COPY files or programs, and extended-query COPY streaming.
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
- Extended query flow: parse, bind, describe, execute, sync, flush, and close.
- Prepared statements and portals.
- Statement descriptions include parameter metadata for explicit type OIDs and supported inferred CRUD parameters.
- Row descriptions are covered for prepared SELECT plus INSERT/UPDATE/DELETE RETURNING flows.
- Named and unnamed statement/portal lifecycle reuse is covered by pgwire tests.
- Positive extended-query execute row limits can suspend portals and emit portal-suspended frames; `max_rows = 0` executes all remaining rows.
- Extended-query protocol errors enter sync-drain mode and return deterministic PostgreSQL-style error fields before ReadyForQuery.
- Row description, data row, command complete, error response, and ready-for-query messages.
- Text and binary format paths covered by tests.

Unsupported or not yet guaranteed:

- Full PostgreSQL backend protocol parity.
- Every optional message type or server parameter exposed by PostgreSQL.
- Function-call protocol messages and stray CopyData/CopyDone/CopyFail messages outside Cassie's supported COPY FROM STDIN flow.
- Exhaustive SQLSTATE parity.

Intentional differences:

- Protocol behavior maps to Cassie execution, catalog, and error surfaces.
- Unsupported features should produce deterministic PostgreSQL-style errors where possible.

## Catalog Compatibility

Supported:

- Catalog introspection required by current Cassie tests.
- Compatibility probes for supported table, schema, column, default, index, constraint, type, and view metadata.
- Virtual catalog views backed by Cassie metadata.
- `information_schema.columns` exposes ordinal position, nullability, type name, UDT name, simple defaults, character length, numeric precision/scale, and datetime precision for supported Cassie types.
- `information_schema.sequences` exposes supported sequence metadata for migration-tool introspection.
- `pg_catalog.pg_attribute`, `pg_catalog.pg_attrdef`, and `pg_catalog.pg_index` expose table/view column metadata, simple default expressions, index uniqueness, primary-index status, and index key ordinals for supported row-store schemas.
- `pg_catalog.pg_namespace`, `pg_catalog.pg_class`, `pg_catalog.pg_attribute`, `pg_catalog.pg_index`, and `pg_catalog.pg_constraint` expose deterministic OID-shaped companion metadata for generic database browser navigation.
- PostgreSQL metadata helper functions used by browser/catalog tools are supported where Cassie can answer deterministically: `pg_get_userbyid`, `quote_ident`, `format_type`, `pg_get_expr`, `has_schema_privilege`, `has_table_privilege`, `pg_table_is_visible`, and `obj_description`.

Unsupported or not yet guaranteed:

- Complete `pg_catalog` parity.
- Complete `information_schema` parity.
- All ORM-specific introspection probes.

## Client Compatibility Matrix

The matrix tracks read-model workflows, not full PostgreSQL server equivalence. A client is supported only for the specific connection, query, catalog, projection, and error-handling behavior listed here.

| Client/workflow | Status | Validated read-model workflows | Validation |
| --- | --- | --- | --- |
| `tokio-postgres` | Supported baseline | Startup without password, simple query, extended prepared query, inferred parameter metadata for supported CRUD shapes, DDL/DML round trip, `ON CONFLICT`, foreign-key errors, NOT NULL/unique SQLSTATE metadata, recursive CTEs, syntax-error recovery, selected catalog metadata | Default `cargo test --locked --test compatibility_matrix` |
| `psql` | Experimental opt-in | Non-interactive connection, simple DDL/DML, simple SELECT output, and operational smoke usage against pgwire | Ignored `should_validate_psql_read_model_probe_when_enabled`; run `CASSIE_RUN_PSQL_COMPAT=1 cargo test --locked --test compatibility_matrix should_validate_psql_read_model_probe_when_enabled -- --ignored --nocapture` with local `psql` installed. Set `CASSIE_PSQL_BIN` to override the binary. |
| `sqlx` | Untested/planned | Prepared read queries, connection pooling, compile-time or offline query checks for supported SQL, catalog probes used by migrations | No automated probe yet |
| `diesel` | Untested/planned | Projection-table reads and supported schema metadata where Diesel does not require unsupported PostgreSQL catalog parity | No automated probe yet |
| `prisma` | Experimental opt-in | `prisma db pull --print` introspection for compatible row-store tables through Cassie's PostgreSQL wire and catalog behavior; generated artifacts stay in a temp directory/stdout | Ignored `should_validate_prisma_introspection_probe_when_enabled`; install the Prisma CLI, then run `CASSIE_RUN_PRISMA_COMPAT=1 cargo test --locked --test compatibility_matrix should_validate_prisma_introspection_probe_when_enabled -- --ignored --nocapture`. Set `CASSIE_PRISMA_BIN` to override the binary. The probe uses `prisma db pull --schema <temp-schema.prisma> --url <cassie-pgwire-url> --print`. |
| `SQLAlchemy` | Experimental opt-in | SQLAlchemy Core connection startup, dialect metadata probes, catalog query, simple SELECT, bound-parameter read query, DDL/DML smoke, unique-violation SQLSTATE, and missing-relation SQLSTATE where generated SQL stays inside Cassie's supported surface and native hstore integration is disabled | Ignored `should_validate_sqlalchemy_read_model_probe_when_enabled`; install Python packages `SQLAlchemy` and `psycopg`, then run `CASSIE_RUN_SQLALCHEMY_COMPAT=1 cargo test --locked --test compatibility_sqlalchemy should_validate_sqlalchemy_read_model_probe_when_enabled -- --ignored --nocapture`. Set `CASSIE_SQLALCHEMY_PYTHON` to override the Python binary. The probe uses `use_native_hstore=False`. |
| `pgAdmin4` | Experimental/manual smoke | Connection registration, database/schema browser, table/view/index/constraint metadata inspection, and table-data browsing for supported schemas through PostgreSQL-compatible pgwire and catalog behavior | Automated Rust coverage validates generic pgAdmin-style catalog/browser queries. Manual smoke: start Cassie with pgwire, register a PostgreSQL server in pgAdmin4 using the Cassie host/port/database/user, browse `Databases` -> `postgres` -> `Schemas` -> `public`, inspect `Tables`, `Views`, `Indexes`, `Constraints`, and run table-data browsing for a supported table. |
| Common migration tools | Experimental/documented | Supported DDL through pgwire: schemas, tables, constraints, indexes, views, simple sequences, `SERIAL`/`BIGSERIAL`, `nextval(...)` defaults, and basic `ALTER COLUMN` default/nullability changes that map to Cassie SQL | Use tool-specific dry runs against a disposable Cassie node; advanced PostgreSQL migration features remain unsupported unless documented separately |

Phase 11 ORM and tooling smoke-probe depth is closed for the current slice with opt-in psql, SQLAlchemy Core, and Prisma CLI probes plus a documented pgAdmin4 manual smoke path.
The default suite remains deterministic and dependency-free beyond Rust dependencies; psql, SQLAlchemy, and Prisma probes require explicit environment variables and local tools.
sqlx, diesel, broader reflection, native extension integration, database-tool automation, and migration-tool automation remain planned compatibility depth rather than implied support.

Unsupported or out-of-scope for client compatibility:

- PostgreSQL server parity checks that require complete `pg_catalog` or `information_schema` behavior.
- Client workflows requiring extensions, native hstore integration, triggers, PL/pgSQL, stored-procedure business logic, LISTEN/NOTIFY, unsupported COPY variants, table inheritance, partitions, logical replication, advisory locks, two-phase commit, or PostgreSQL storage parameters.
- Administrative tool workflows requiring PostgreSQL maintenance, extension-management, server-log, replication, tablespace, role-management, or monitoring catalog parity beyond Cassie's documented virtual catalog surface.
- ORM-generated OLTP workloads that depend on distributed transactions, row-locking semantics, trigger business logic, broad reflection parity, or PostgreSQL optimizer behavior.
- Migration diffs that assume PostgreSQL-owned physical storage, operator classes, collations, or extension-managed metadata.

Future client probes should stay isolated from the default suite unless the dependency is lightweight, deterministic, and available without external services.

## ORM And Toolkit Compatibility Contract

Cassie compatibility work targets PostgreSQL behavior, not per-client detection or adapters. A driver, ORM, migration tool, query builder, or database utility is considered compatible only when it works against Cassie through the PostgreSQL wire protocol by changing the connection string.

Repository-owned compatibility coverage:

- Rust tests for SQL parsing, binding, planning, execution, catalog metadata, pgwire protocol behavior, SQLSTATEs, and transaction semantics.
- Lightweight smoke fixtures for representative ecosystem workflows when dependencies are deterministic enough for local validation.
- Documentation of supported, partial, unsupported, and Cassie-specific behavior.

External-suite coverage:

- Full ORM/toolkit integration suites may live outside this repository.
- External suites must start Cassie over pgwire, provide the exact Cassie build/version, pin client/tool versions, and report failures as PostgreSQL behavior gaps.
- Cassie should not add client-name detection, query-shape hacks, or framework-specific compatibility branches to satisfy external suites.

## Ecosystem Compatibility Matrix

Status definitions:

| Status | Meaning |
| --- | --- |
| Supported | Covered by automated Cassie-owned tests and expected to remain stable. |
| Smoke | A narrow representative workflow is covered; broader official suites are external or pending. |
| Partial | Cassie implements some PostgreSQL behavior required by this ecosystem, but known gaps remain. |
| Planned | Compatibility is not validated yet. |

| Ecosystem | Clients/tools | Status | Primary gaps before support |
| --- | --- | --- | --- |
| TypeScript/JavaScript | Prisma, Drizzle, Kysely, Knex, TypeORM, MikroORM, Sequelize, node-postgres, postgres.js | Partial | Full catalog parity, migration DDL breadth, generated/default metadata, identity support, broader client-specific workflow validation |
| .NET | Npgsql, EF Core, Dapper, RepoDB, Linq2Db | Planned | Npgsql protocol probes, EF catalog/scaffolding metadata, migration DDL, binary encodings |
| Python | SQLAlchemy, Alembic, psycopg, asyncpg, Django ORM, Tortoise ORM | Partial | Broader reflection metadata, Alembic/Django migration DDL, asyncpg protocol coverage |
| Go | pgx, database/sql, GORM, sqlc, Bun, Ent, SQLBoiler, Jet | Planned | pgx protocol matrix, scanner type mappings, migration/reflection metadata |
| Java | Hibernate, jOOQ, Spring Data JDBC, Spring Data JPA, MyBatis | Planned | JDBC metadata, generated schema DDL, identifier/case behavior, transaction edge cases |
| Ruby | ActiveRecord, Sequel, pg | Planned | Rails schema dumps/migrations, catalog/reflection metadata, association queries |
| PHP | Laravel Eloquent, Doctrine ORM, PDO PostgreSQL | Planned | PDO smoke, Doctrine metadata, Laravel migration DDL |
| Rust | tokio-postgres, sqlx, diesel, sea-orm | Partial | sqlx/diesel/sea-orm migration and metadata probes beyond tokio-postgres baseline |
| Elixir | Ecto, Postgrex | Planned | Postgrex protocol smoke, Ecto migrations/reflection |
| C/C++ | libpq, libpqxx | Planned | libpq/libpqxx connection, prepared statements, result metadata, error handling |
| Database tools | psql, pgAdmin4, pg_dump, pg_restore, pgbench, schema diff and monitoring tools | Partial | pgAdmin4 browser probes, dump/restore format support, pgbench workload compatibility, deeper system catalog parity |
| Migration tools | Flyway, Liquibase, Goose, Atlas, Alembic, Prisma Migrate, EF Core Migrations, Rails Migrations, Django Migrations | Partial | `ALTER TABLE` breadth, sequence/identity/default behavior, constraint/index metadata, shadow/reflection workflows |

Current foundation fixture:

- Cassie accepts the pipeline application schema used for the first ORM compatibility slice, including quoted identifiers, `JSONB`, `TIMESTAMP(n)`, named table primary-key constraints, composite indexes, and `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY ... REFERENCES ...`.
- Simple named primary-key, unique, check, and foreign-key constraints are persisted in constraint metadata and exposed through `information_schema.table_constraints`, `information_schema.key_column_usage`, `information_schema.referential_constraints`, and `pg_catalog.pg_constraint`.
- Direct foreign-key `CASCADE`, `SET NULL`, `SET DEFAULT`, `NO ACTION`, and `RESTRICT` actions are enforced for parent deletes and key updates when those actions are captured in constraint metadata.
- ORM introspection metadata now includes simple column defaults and pg-catalog attribute/default/index rows for supported tables.
- Migration DDL now includes bare `CREATE SEQUENCE`/`DROP SEQUENCE`, sequence-backed `nextval(...)` defaults, `SERIAL`/`BIGSERIAL` table-column sugar, and `ALTER TABLE ... ALTER COLUMN` set/drop default and set/drop not-null behavior for rows that already satisfy the constraint.
- Extended-query metadata now includes explicit and inferred parameter OIDs for supported CRUD shapes, row descriptions for prepared SELECT and DML RETURNING statements, named/unnamed statement lifecycle coverage, and sync-drain recovery after statement errors.
- Generic database-browser support now includes pgAdmin4-style schema/table/view/index/constraint catalog browsing and supported table-data inspection without client detection.
- Composite constraint fidelity, deferrable constraints, match types, and advanced cyclic/deferred referential-action behavior remain compatibility gaps for full ORM migration diffing.

## Cassie-Specific SQL and APIs

These features are intentionally Cassie-specific:

- Projection source checkpoints, replay metadata, freshness, versioning, swaps, and verification diagnostics.
- `search(field, query)`, `search_score(field, query)`, and `snippet(field, query)`.
- `vector_score`, `vector_distance`, `cosine_distance`, `dot_product`, and `l2_distance`.
- pgvector-style operators implemented by Cassie, including `<=>`, `<->`, and `<#>`.
- `hybrid_score(text_score, vector_score)`.
- `CREATE GRAPH`, `graph_neighbors`, `graph_expand`, and `graph_shortest_path`.
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
