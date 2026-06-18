# Sprint 28 - Auth and Session Identity

Previous: [Sprint 27 - Wire Type Metadata Policy](sprint-27.md)
Next: [Sprint 29 - Binary Pgwire Startup and Auth](sprint-29.md)

## Goal

Define Cassie's V1 role-based identity and authorization posture before real PostgreSQL wire clients connect.

## Requirements

- Define a PostgreSQL-style V1 role model with a bootstrap SA/admin login role and individual user login roles.
- Persist role definitions and per-role credentials using Argon2 password hashes.
- Define database name handling and session identity behavior for authenticated roles.
- Implement `CREATE ROLE`, `ALTER ROLE`, and `DROP ROLE` for login-role lifecycle management.
- Explicitly reject `GRANT`, `REVOKE`, role membership changes, `SET ROLE`, `SET SESSION AUTHORIZATION`, row-level security, security definer functions, and privilege escalation features until they are intentionally added.
- Keep REST authorization deterministic and map authenticated identities onto the same local role model Cassie uses over pgwire.
- Treat external identity providers such as Auth0 as upstream concerns, not wire-protocol auth in V1.

## Acceptance Criteria

- `current_user`, `session_user`, `current_role`, and catalog metadata expose the authenticated role identity.
- The bootstrap admin role and any created login roles are persisted and visible through `pg_catalog.pg_roles`.
- Password verification uses Argon2-backed per-role credentials and fails deterministically on mismatch.
- Unsupported role-management and privilege features return explicit PostgreSQL-style errors.
- REST auth behavior is deterministic for both admin and non-admin identities.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- `tests/auth.rs` for role/session identity, persisted login roles, password auth, and REST authorization behavior.
- Parser tests for role lifecycle SQL and rejected privilege SQL.
- Catalog tests for `pg_catalog.pg_roles` visibility and session identity metadata.

## Exit Gate

This sprint is complete when auth and session identity behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
