# Sprint 16 - SQL INSERT SELECT

Previous: [Sprint 15 - SQL INSERT VALUES](sprint-15.md)
Next: [Sprint 17 - SQL UPDATE](../sprint-17.md)

## Goal

Support `INSERT INTO ... SELECT ...` when source rows can be deterministically mapped into the target row schema.

## Requirements

- Execute `INSERT SELECT` through the same planner, executor, validation, and row blob write path as `INSERT VALUES`.
- Validate source projection count and target columns before writing any rows.
- Apply defaults, constraints, vector validation, and catalog type checks to every inserted row.
- Support deterministic `RETURNING`.
- Keep writable CTEs and advanced insert forms explicitly unsupported.

## Acceptance Criteria

- Compatible `INSERT SELECT` writes all source rows as row blobs.
- Incompatible source/target shapes fail deterministically before partial writes.
- `RETURNING` rows are deterministic.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Planner/executor tests for compatible and incompatible `INSERT SELECT`.
- Integration SQL tests for source-to-target round trips.
- Validation parity tests for constraints/defaults/vector fields.

## Exit Gate

This sprint is complete when `INSERT SELECT` behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
