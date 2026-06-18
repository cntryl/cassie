# Sprint 17 - SQL UPDATE

Previous: [Sprint 16 - SQL INSERT SELECT](sprint-16.md)
Next: [Sprint 18 - SQL DELETE](sprint-18.md)

## Goal

Support `UPDATE ... SET ... WHERE ...` against row blob storage with deterministic validation and `RETURNING`.

## Requirements

- Plan `UPDATE` as an explicit logical mutation operation.
- Evaluate predicates through the existing expression/filter path.
- Rewrite only matching rows as row blobs while preserving row IDs.
- Reapply constraints, defaults where relevant, vector validation, and catalog type checks to updated payloads.
- Support deterministic `RETURNING` for updated rows.
- Keep multi-table update forms and `UPDATE FROM` explicitly unsupported.

## Acceptance Criteria

- Updates mutate only matching rows.
- Failed validation does not silently fallback or partially succeed.
- `RETURNING` rows and metadata are deterministic.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Planner tests for update mutation plans.
- Executor/integration tests for matching predicates, non-matching rows, validation failures, and `RETURNING`.
- Storage tests for row ID preservation and row blob rewrites.

## Exit Gate

This sprint is complete when SQL update behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
