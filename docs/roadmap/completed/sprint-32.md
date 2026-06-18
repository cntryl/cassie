# Sprint 32 - Extended Query Portals and Recovery

Previous: [Sprint 31 - Extended Query Parse/Bind/Execute](sprint-31.md)
Next: [Sprint 33 - Compatibility Matrix and CI Gate](sprint-33.md)

## Goal

Complete extended query session behavior with portals, close messages, and protocol error recovery.

## Requirements

- Implement portals and close behavior for prepared statements and portals.
- Return deterministic parameter, row, and command metadata.
- Ensure protocol errors recover to ready-for-query where PostgreSQL clients expect recovery.
- Define explicit unsupported behavior for COPY, notifications, cancel, and advanced protocol features.

## Acceptance Criteria

- Close removes prepared statements and portals from the session.
- Protocol errors return ready-for-query where expected.
- Unsupported protocol features produce PostgreSQL-style errors.
- Full `cargo test`, `cargo build`, Clippy, and touched-test validation pass.

## Tests

- Pgwire tests for portal lifecycle, close behavior, error recovery, and unsupported feature errors.
- Driver smoke tests for prepared statement workflows where available.

## Exit Gate

This sprint is complete when extended query portal and recovery behavior is validator-clean, full `cargo test` passes, `cargo build` passes, and Clippy is clean with warnings denied.
