# Phase 11 Issue 01: Catalog Metadata Baseline For ORM Introspection

## Status

Open.

## Goal

Expand PostgreSQL-compatible catalog metadata used by Prisma-style introspection and pgAdmin4 schema browsing without client-specific behavior.

## Dependencies

- Phase 10 is closed.
- `docs/postgres-compatibility.md` documents the current partial catalog and tooling compatibility surface.
- Existing catalog views for schemas, tables, columns, indexes, constraints, types, roles, and user views remain working.

## Implementation Plan

1. Add failing catalog tests for PostgreSQL-shaped metadata that ORM/tooling introspection commonly reads and Cassie already has enough catalog data to answer.
2. Prioritize metadata in this order:
   - `information_schema.columns`: table/schema/name, ordinal position, nullability, data type, udt name, character length, numeric precision/scale, datetime precision, and column default where Cassie stores one.
   - `information_schema.table_constraints`, `key_column_usage`, and `referential_constraints`: stable names, constraint type, column ordinal, referenced table/column, and referential actions.
   - `pg_catalog.pg_class`, `pg_namespace`, `pg_attribute`, `pg_type`, `pg_index`, `pg_constraint`, and `pg_attrdef`: stable rows for user tables, views, indexes, primary keys, unique constraints, foreign keys, checks, and defaults.
3. Keep catalog rows deterministic by schema/name/ordinal ordering.
4. Represent unsupported PostgreSQL-only fields as PostgreSQL-compatible null/default values only when that is the documented behavior for absent metadata.
5. Update `docs/postgres-compatibility.md` with newly covered catalog behavior and remaining gaps.

## Acceptance Criteria

- Catalog tests prove the baseline metadata for a table with defaults, not-null fields, primary key, unique constraint, check constraint, foreign key, and index.
- Existing catalog and compatibility tests remain green.
- No client-name detection or Prisma/pgAdmin4-specific query branching is added.
- Files touched for new tests stay below 1,000 lines.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
```

