# Sprint 26 - Type Catalog and SQL Casts

Previous: [Sprint 25 - Catalog Compatibility Probes](sprint-25.md)
Next: [Sprint 27 - Wire Type Metadata Policy](sprint-27.md)

## Goal

Define Cassie's V1 PostgreSQL-compatible type catalog and deterministic SQL casts.

## Requirements

- Define V1 type catalog entries and OIDs for supported Cassie scalar, text, boolean, integer, float, numeric-compatible, JSON, UUID, timestamp/date/time, vector, null, and limited array values.
- Implement deterministic casts through `CAST(...)` and `::type`.
- Align REST JSON values and SQL values where they round trip through row storage.
- Return explicit unsupported errors for unsupported type families.

## Acceptance Criteria

- Supported types parse, bind, execute, cast, store, and retrieve deterministically.
- Type catalog rows are stable.
- Unsupported casts fail deterministically.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- `tests/types.rs` for casts, nulls, JSON, UUID, time values, arrays, and vector metadata.
- Integration SQL tests for type round trips through row blob storage.
- Catalog introspection tests for type rows and OIDs.

## Exit Gate

This sprint is complete when type catalog and cast behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
