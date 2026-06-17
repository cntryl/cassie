# Sprint 21 - PostgreSQL Extended Query Protocol and Client Compatibility

Previous: [Sprint 20 - Real PostgreSQL Wire Protocol Core](sprint-20.md)  
Next: [Sprint 22 - REST, Operations, Packaging, and V1 Release Gate](sprint-22.md)

## Goal

Complete PostgreSQL extended query protocol support and prove Cassie can serve practical driver, ORM, migration, and BI workflows through the real wire path.

## Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` documents/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## Requirements

- Implement parse, bind, describe, execute, sync, and close messages.
- Implement session-local prepared statements and portals.
- Support parameter values from PostgreSQL wire bind messages.
- Return deterministic parameter, row, and command metadata.
- Keep parameterized SQL execution routed through the same planner and executor as direct SQL.
- Define explicit unsupported behavior for transactions, COPY, DDL outside V1, notifications, cancel, and advanced PostgreSQL features.
- Ensure protocol errors recover to ready-for-query where PostgreSQL clients expect recovery.
- Add compatibility smoke tests for common drivers and at least one ORM or migration workflow.

## Acceptance Criteria

- Prepared statement lifecycle tests pass through real wire messages.
- `psql`, one Rust driver, one Python or Node driver, and one ORM or migration workflow can connect and query.
- Parameterized SQL executes through the same planner/executor path as REST/native calls.
- Protocol errors return ready-for-query where PostgreSQL clients expect recovery.
- Close removes prepared statements and portals from the session.
- Unsupported extended protocol features produce PostgreSQL-style errors.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/pgwire.rs`: parse, bind, describe, execute, sync, and close lifecycle.
- `tests/pgwire.rs`: prepared statement reuse and portal lifecycle.
- `tests/pgwire.rs`: bind parameter conversion to Cassie values.
- `tests/pgwire.rs`: unsupported feature errors recover to ready-for-query.
- Driver smoke tests for selected Rust and Python or Node clients.
- ORM or migration smoke test with a clearly documented supported workflow.

## Exit Gate

This sprint is complete when extended query protocol support works through real PostgreSQL wire messages, practical driver compatibility smoke tests pass, touched tests are validator-clean, `cargo build` passes, and Clippy is clean with warnings denied.
