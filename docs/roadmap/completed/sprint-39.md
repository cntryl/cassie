# Sprint 39 - Schema DDL Breadth and Index Variants

Previous: [Sprint 38 - SQL Type Coverage and Metadata Fidelity](sprint-38.md)
Next: [Sprint 40 - Benchmark Harness and Output Contract](../sprint-40.md)

## Goal

Close the remaining DDL gaps around namespace lifecycle, column evolution, and broader index definitions.

## Requirements

- Add `DROP SCHEMA` and `ALTER SCHEMA` support with deterministic namespace cleanup and rename behavior.
- Add `ALTER TABLE RENAME COLUMN`.
- Add composite index support and keep unsupported index forms explicit when they remain out of scope.
- Keep catalog metadata, plan-cache invalidation, and restart hydration consistent across schema and index changes.
- Preserve deterministic errors for database-level DDL if Cassie remains a single-database engine.

## Acceptance Criteria

- Namespace lifecycle operations behave consistently before and after restart.
- Column rename preserves data access and catalog metadata.
- Composite indexes create, surface, and drop correctly.
- Unsupported DDL stays explicit and deterministic.
- The sprint exits with touched-test validation, `cargo build`, and Clippy green.

## Tests

- Root tests for schema rename/drop and column rename coverage.
- Index tests for composite indexes and metadata visibility.
- Restart hydration tests for schema and index objects.
- Compatibility tests for schema and index DDL through pgwire.

## Exit Gate

This sprint is complete when the DDL and index suite is green, touched tests validate, `cargo build` passes, and Clippy is clean with warnings denied.
