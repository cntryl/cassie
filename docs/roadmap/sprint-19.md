# Sprint 19 - Transaction Control Basics

Previous: [Sprint 18 - SQL DELETE](sprint-18.md)
Next: [Sprint 20 - Transaction Write Semantics](sprint-20.md)

## Goal

Define and implement basic session transaction control that practical PostgreSQL clients can reason about.

## Requirements

- Parse and execute `BEGIN`, `COMMIT`, and `ROLLBACK`.
- Define autocommit behavior for single statements.
- Add deterministic session transaction state.
- Return explicit unsupported errors for savepoints, isolation-level changes, two-phase commit, advisory locks, and distributed transaction semantics.

## Acceptance Criteria

- Autocommit statements remain visible after success.
- Transaction control statements transition session state deterministically.
- Unsupported transaction controls fail deterministically.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Parser tests for transaction control and unsupported forms.
- Executor/session tests for state transitions.
- Integration SQL tests for autocommit visibility.

## Exit Gate

This sprint is complete when transaction control basics are validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
