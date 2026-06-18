# Sprint 20 - Transaction Write Semantics

Previous: [Sprint 19 - Transaction Control Basics](sprint-19.md)
Next: [Sprint 21 - Relational Predicates and Scalar SQL](../sprint-21.md)

## Goal

Make writes inside transactions deterministic for row blob storage, including commit, rollback, read-your-writes, and failed transaction recovery.

## Requirements

- Define Midge transaction boundaries for reads and writes within Cassie sessions.
- Support committed transaction persistence and rollback without persisted writes.
- Support read-your-writes behavior inside a session transaction.
- Move errors inside transactions into a deterministic failed state until rollback.
- Keep runtime metrics and plan cache behavior correct across transaction boundaries.

## Acceptance Criteria

- Committed writes persist.
- Rolled-back writes do not persist.
- Failed transactions reject further work until rollback.
- Full `cargo test`, `cargo build`, Clippy, and touched-test validation pass.

## Tests

- Executor/integration tests for commit, rollback, read-your-writes, and failed state.
- Storage tests confirming row blob family routing inside transactions.
- Metrics/plan-cache regression tests for transaction boundaries.

## Exit Gate

This sprint is complete when transaction write behavior is validator-clean, full `cargo test` passes, `cargo build` passes, and Clippy is clean with warnings denied.
