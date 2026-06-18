# Sprint 18 - SQL DELETE

Previous: [Sprint 17 - SQL UPDATE](sprint-17.md)
Next: [Sprint 19 - Transaction Control Basics](sprint-19.md)

## Goal

Support `DELETE FROM ... WHERE ...` against row blob storage with deterministic row removal and `RETURNING`.

## Requirements

- Plan `DELETE` as an explicit logical mutation operation.
- Evaluate predicates through the existing expression/filter path.
- Delete only matching row blob and legacy-compatible keys.
- Support deterministic `RETURNING` for deleted rows.
- Keep advanced delete forms explicitly unsupported.

## Acceptance Criteria

- Deletes remove only matching rows.
- Deleted rows cannot reappear through legacy fallback.
- `RETURNING` rows and metadata are deterministic.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Planner tests for delete mutation plans.
- Executor/integration tests for filtered deletes, no-op deletes, and `RETURNING`.
- Storage tests for row blob and legacy key cleanup.

## Exit Gate

This sprint is complete when SQL delete behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
