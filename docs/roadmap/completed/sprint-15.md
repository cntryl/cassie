# Sprint 15 - SQL INSERT VALUES

Previous: [Sprint 14 - Row Storage Rebuild and Decode Controls](sprint-14.md)
Next: [Sprint 16 - SQL INSERT SELECT](sprint-16.md)

## Goal

Support `INSERT INTO ... VALUES ...` through the primary SQL interface, writing row blobs through the same validation path used by REST ingest.

## Requirements

- Plan `INSERT INTO ... VALUES ...` as an explicit logical mutation operation.
- Execute inserts through existing Cassie write validation: generated IDs, defaults, constraints, catalog type checks, vector validation, and row blob persistence.
- Support explicit column lists and table-column order when the column list is omitted.
- Support simple `RETURNING` for inserted rows.
- Keep `ON CONFLICT` and advanced insert forms explicitly unsupported.

## Acceptance Criteria

- SQL inserts create retrievable row blobs in `cf1`.
- Constraint, default, vector, and type behavior matches REST ingest.
- `RETURNING` rows and metadata are deterministic.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Parser/binder regression tests for `INSERT VALUES` and unsupported `ON CONFLICT`.
- Planner tests for mutation plan shape.
- Executor/integration tests for inserts, generated IDs, validation, and `RETURNING`.
- Storage tests proving inserted rows use row blobs in `cf1`.

## Exit Gate

This sprint is complete when `INSERT VALUES` behavior is validator-clean, targeted DML tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
