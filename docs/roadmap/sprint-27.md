# Sprint 27 - Wire Type Metadata Policy

Previous: [Sprint 26 - Type Catalog and SQL Casts](sprint-26.md)
Next: [Sprint 28 - Auth and Session Identity](sprint-28.md)

## Goal

Align SQL result metadata with the type catalog so PostgreSQL wire encoding can expose stable row descriptions.

## Requirements

- Define text and binary wire encoding policy for supported V1 types.
- Ensure row descriptions expose correct OIDs, widths, format codes, and nullability where available.
- Align catalog type rows with pgwire row metadata.
- Keep unsupported wire formats deterministic.

## Acceptance Criteria

- Row description metadata matches catalog type metadata.
- Null handling is stable across SQL, REST, and pgwire-facing metadata.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Type metadata tests for row descriptions and OIDs.
- Pgwire metadata tests after real protocol support lands, with direct metadata tests in this sprint.

## Exit Gate

This sprint is complete when wire type metadata policy is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
