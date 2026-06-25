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

1. `issues/phase-11/issue-01.md`
2. `issues/phase-11/issue-02.md`
3. `issues/phase-11/issue-03.md`
4. `issues/phase-11/issue-04.md`
5. `issues/phase-11/issue-05.md`

