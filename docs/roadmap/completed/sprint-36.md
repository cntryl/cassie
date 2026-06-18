# Sprint 36 - Stored Procedure Execution and CALL Semantics

Previous: [Sprint 35 - User-Defined Views and View Expansion](completed/sprint-35.md)
Next: [Sprint 37 - Common Scalar Functions](completed/sprint-37.md)

## Goal

Turn stored procedures into executable SQL units instead of metadata-only entries so `CALL` runs procedure bodies with parameter binding and predictable error handling.

## Requirements

- Bind `CALL` arguments against stored-procedure metadata.
- Execute procedure bodies through the existing parser, planner, and executor pipeline.
- Keep procedure definitions persisted and hydrated separately from UDFs.
- Preserve session-local scope for procedure execution while keeping the procedure catalog global.
- Invalidate the shared plan cache when procedures are created, dropped, or replaced.
- Return deterministic errors for unsupported procedural constructs, including PL blocks and transaction control if they remain out of scope.

## Acceptance Criteria

- `CREATE PROCEDURE`, `CALL`, and `DROP PROCEDURE` work end-to-end with arguments.
- Procedure bodies can query or mutate data through the normal SQL stack.
- Procedure errors propagate to the caller deterministically.
- Procedure definitions survive restart and remain visible in catalog metadata.
- The sprint exits with touched-test validation, `cargo build`, and Clippy green.

## Tests

- New root tests for procedure execution and parameter binding.
- Compatibility matrix coverage for `CALL` through tokio-postgres.
- Restart hydration tests for stored procedures.
- Error-path tests for unsupported procedural syntax.

## Exit Gate

This sprint is complete when procedure execution is green, touched tests validate, `cargo build` passes, and Clippy is clean with warnings denied.
