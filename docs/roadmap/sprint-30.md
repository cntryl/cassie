# Sprint 30 - Binary Pgwire Simple Query

Previous: [Sprint 29 - Binary Pgwire Startup and Auth](completed/sprint-29.md)
Next: [Sprint 31 - Extended Query Parse/Bind/Execute](sprint-31.md)

## Goal

Implement PostgreSQL binary simple-query protocol routed through `Cassie::execute_sql`.

## Requirements

- Implement simple query message handling.
- Emit PostgreSQL-compatible row description, data row, command complete, error response, and ready-for-query messages.
- Map Cassie scalar values through the type metadata policy.
- Preserve deterministic ready-state transitions.
- Retire or convert simplified simple-query protocol tests.

## Acceptance Criteria

- Simple-query wire execution returns the same rows as direct SQL execution.
- `psql` can run a simple `SELECT` when available.
- Errors return PostgreSQL-style error responses and recover to ready-for-query where appropriate.
- Full `cargo test`, `cargo build`, Clippy, and touched-test validation pass.

## Tests

- Pgwire tests for simple query lifecycle, row metadata, data rows, command complete, errors, and ready-for-query.
- Optional real-client smoke test for `psql` or libpq when available.

## Exit Gate

This sprint is complete when binary simple-query behavior is validator-clean, full `cargo test` passes, `cargo build` passes, and Clippy is clean with warnings denied.
