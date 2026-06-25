# Phase 11 Issue 02: Migration DDL Compatibility Basics

## Status

Open.

## Goal

Add PostgreSQL-compatible DDL needed by common migration tools for simple application schemas, starting with sequence/default behavior that can support Prisma-style migrations.

## Dependencies

- `issues/phase-11/issue-01.md` is complete.
- Catalog metadata exposes defaults and constraints well enough for migration tools to inspect the result of supported DDL.

## Implementation Plan

1. Add failing parser/binder/executor tests for the first supported migration DDL set:
   - `CREATE SEQUENCE`, `DROP SEQUENCE`, and `nextval(...)` defaults for simple integer identifiers.
   - `SERIAL`/`BIGSERIAL` compatibility as table-column sugar for integer field plus sequence-backed default.
   - `ALTER TABLE ... ALTER COLUMN ... SET DEFAULT` and `DROP DEFAULT`.
   - `ALTER TABLE ... ALTER COLUMN ... SET NOT NULL` and `DROP NOT NULL` where existing rows satisfy the constraint.
2. Persist sequence and default metadata through restart using the existing catalog/Midge metadata patterns.
3. Expose sequence/default metadata through the catalog views covered by Issue 01.
4. Reject unsupported sequence options deterministically instead of accepting syntax with ignored behavior.
5. Update compatibility docs with supported DDL and remaining migration gaps.

## Acceptance Criteria

- Supported DDL round-trips through SQL execution and restart.
- Defaults apply to `INSERT` and `INSERT ... SELECT` consistently with existing default behavior.
- Migration metadata is visible through catalog tests.
- Unsupported sequence/identity options return deterministic errors.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
```

