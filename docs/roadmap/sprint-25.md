# Sprint 25 - Catalog Compatibility Probes

Previous: [Sprint 24 - PostgreSQL Catalog Basics](completed/sprint-24.md)
Next: [Sprint 26 - Type Catalog and SQL Casts](sprint-26.md)

## Goal

Handle common PostgreSQL metadata probes used by clients, ORMs, and migration tools.

## Requirements

- Support `version()`, `current_schema()`, and `current_database()`.
- Support `SHOW` and selected `SET` no-op or validation behavior.
- Add compatibility fixtures for common metadata probes.
- Keep unsupported catalog/session features deterministic.

## Acceptance Criteria

- Common metadata probes return stable rows.
- Unsupported probes do not crash clients.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Catalog introspection tests for context functions, `SHOW`, and selected `SET`.
- Compatibility fixtures for ORM and migration metadata probes.

## Exit Gate

This sprint is complete when catalog compatibility probes are validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
