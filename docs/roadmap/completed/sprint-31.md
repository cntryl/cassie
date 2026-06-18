# Sprint 31 - Extended Query Parse/Bind/Execute

Previous: [Sprint 30 - Binary Pgwire Simple Query](sprint-30.md)
Next: [Sprint 32 - Extended Query Portals and Recovery](../sprint-32.md)

## Goal

Implement the core PostgreSQL extended query flow with session-local prepared statements.

## Requirements

- Implement parse, bind, describe, execute, and sync messages.
- Keep prepared statements session-local and separate from the shared plan cache.
- Support bind parameter conversion to Cassie values.
- Route parameterized SQL through the same planner and executor as direct SQL.

## Acceptance Criteria

- Prepared statement lifecycle tests pass through real binary wire messages.
- Parameterized SQL returns deterministic rows and metadata.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Pgwire tests for parse, bind, describe, execute, sync, prepared statement reuse, and bind parameter conversion.
- Driver smoke tests where available.

## Exit Gate

This sprint is complete when core extended query behavior is validator-clean, targeted pgwire tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
