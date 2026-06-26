# Phase 11: ORM And Database Tooling Compatibility

Phase 11 is the active compatibility gate after the Phase 10 performance rebaseline.

The goal is broad PostgreSQL ecosystem compatibility for application frameworks, ORMs, migration tools, query builders, and database administration tools. Compatibility work must improve PostgreSQL behavior through pgwire, SQL, catalog metadata, SQLSTATEs, and transaction semantics rather than detecting individual clients.

## Core Rule

Implement PostgreSQL behavior, not ORM-specific hacks.

Every slice must benefit more than one PostgreSQL client when practical, preserve existing pgwire clients, and keep external full integration suites outside this repository unless a deterministic smoke probe is explicitly added.

## Phase Sequence

1. Prisma and pgAdmin4-facing catalog metadata baseline.
2. Migration DDL basics for PostgreSQL-compatible schema tools.
3. Prepared statement and parameter metadata compatibility depth.
4. pgAdmin4 browser and table-data workflow support.
5. Opt-in ORM and database-tooling smoke probes.

## Required Gates

- Phase 10 is closed in `issues/phase-10/README.md`.
- `docs/postgres-compatibility.md` is the client/tooling compatibility surface.
- `docs/production-readiness.md` keeps client/tooling compatibility as stable-candidate or experimental until evidence supports stronger claims.
- Full Prisma and broader ORM/tooling integration suites may live outside this repository.

## Non-Goals

- No Prisma-specific, pgAdmin4-specific, or client-name detection branches.
- No second storage abstraction above Midge.
- No PostgreSQL extension, replication, tablespace, server-log, LISTEN/NOTIFY, or role-management parity unless a future issue explicitly scopes it.
- No production-ready promotion from smoke tests alone.

## Open Issues

Resolve Phase 11 issues in order:

1. `issues/phase-11/issue-05.md`

## Archived Issue Summaries

- Issue 01, catalog metadata baseline for ORM introspection, closed 2026-06-25. `information_schema.columns` now exposes nullability, UDT names, simple defaults, character lengths, numeric precision/scale, and datetime precision; `pg_catalog.pg_attribute`, `pg_catalog.pg_attrdef`, and `pg_catalog.pg_index` now expose supported attribute/default/index metadata for ORM and database-tooling introspection without client-specific behavior.
- Issue 02, migration DDL compatibility basics, closed 2026-06-25. Cassie now supports bare `CREATE SEQUENCE`/`DROP SEQUENCE`, durable sequence-backed `nextval(...)` defaults, `SERIAL`/`BIGSERIAL` table-column sugar, and basic `ALTER TABLE ... ALTER COLUMN` set/drop default and set/drop not-null operations, with sequence metadata exposed through PostgreSQL-compatible catalog views and unsupported sequence options rejected deterministically.
- Issue 03, prepared statement and parameter metadata depth, closed 2026-06-25. Extended-query pgwire coverage now verifies explicit and inferred parameter descriptions, prepared SELECT and DML RETURNING row descriptions, named and unnamed statement/portal lifecycle behavior, deterministic SQLSTATE/error fields, and ReadyForQuery recovery after extended-query errors without client-specific protocol branches.
- Issue 04, pgAdmin4 browser workflow support, closed 2026-06-26. Generic PostgreSQL catalog/browser support now exposes deterministic OID-shaped companion metadata, browser helper functions, pgAdmin4-style schema/table/view/index/constraint queries, and supported table-data inspection without client-specific detection; unsupported PostgreSQL administrative areas remain documented.
