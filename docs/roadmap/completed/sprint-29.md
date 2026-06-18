# Sprint 29 - Binary Pgwire Startup and Auth

Previous: [Sprint 28 - Auth and Session Identity](../sprint-28.md)
Next: [Sprint 30 - Binary Pgwire Simple Query](sprint-30.md)

## Goal

Replace simplified startup/auth handling with real PostgreSQL frontend/backend binary framing for startup and authentication.

## Requirements

- Implement PostgreSQL startup packet parsing.
- Define and implement SSL request behavior for V1.
- Implement authentication negotiation compatible with practical clients.
- Emit PostgreSQL-compatible auth, error, and ready-state messages for startup/auth paths.
- Retire or convert simplified startup/auth protocol tests.

## Acceptance Criteria

- `psql` or a libpq-style client can connect and authenticate when available.
- Unsupported startup options fail with PostgreSQL-style errors without crashing the session.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Pgwire tests for binary startup packet parsing, SSL request behavior, auth success, auth failure, and startup errors.
- Optional real-client smoke test when client tools are available.

## Exit Gate

This sprint is complete when binary startup/auth behavior is validator-clean, targeted pgwire tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
