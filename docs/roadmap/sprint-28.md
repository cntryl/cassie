# Sprint 28 - Auth and Session Identity

Previous: [Sprint 27 - Wire Type Metadata Policy](completed/sprint-27.md)
Next: [Sprint 29 - Binary Pgwire Startup and Auth](completed/sprint-29.md)

## Goal

Define Cassie's V1 identity and authorization posture before real PostgreSQL wire clients connect.

## Requirements

- Define V1 role model, default admin role, database name handling, and session identity behavior.
- Define password storage/configuration and secret-handling expectations for single-container V1.
- Implement or explicitly reject `CREATE ROLE`, `DROP ROLE`, `GRANT`, `REVOKE`, row-level security, security definer functions, and privilege escalation features.
- Ensure REST admin authorization behavior is deterministic.

## Acceptance Criteria

- Session identity is visible to supported context functions and catalog metadata.
- Unsupported role and privilege features return explicit PostgreSQL-style errors.
- REST auth behavior is deterministic.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- `tests/auth.rs` for role/session identity and auth behavior.
- Parser tests for role and privilege SQL.
- REST tests for authorization posture.

## Exit Gate

This sprint is complete when auth and session identity behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
