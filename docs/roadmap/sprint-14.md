# Sprint 14 - Transactions and Session Semantics

Previous: [Sprint 13 - SQL DML and Mutation Semantics](sprint-13.md)  
Next: [Sprint 15 - Relational SQL Expansion](sprint-15.md)

## Goal

Define and implement V1 transaction and session behavior that is practical for PostgreSQL clients while remaining honest about Cassie's single-node, Midge-backed execution model.

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

- Support autocommit semantics for single statements.
- Support `BEGIN`, `COMMIT`, and `ROLLBACK` with deterministic session state.
- Define Midge transaction boundaries for reads and writes within Cassie sessions.
- Implement failed-transaction state and recovery behavior compatible with practical PostgreSQL clients.
- Support read-your-writes behavior inside a session transaction.
- Return explicit unsupported errors for savepoints, isolation-level changes, two-phase commit, advisory locks, and distributed transaction semantics unless implemented.
- Ensure transaction state integrates with pgwire ready-for-query states.
- Ensure runtime metrics and plan cache behavior remain correct across transaction boundaries.

## Acceptance Criteria

- Autocommit statements are visible after success.
- Committed transactions persist writes.
- Rolled-back transactions do not persist writes.
- Errors inside transactions move the session to deterministic failed state until rollback.
- Unsupported transaction features return PostgreSQL-style errors.
- Pgwire can expose idle, in-transaction, and failed-transaction ready states after protocol implementation.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse transaction control statements.
- `tests/executor.rs`: autocommit, commit, rollback, and failed transaction behavior.
- `tests/integration_sql.rs`: transaction visibility and rollback persistence tests.
- `tests/midge_cf_layout.rs`: transaction writes preserve family routing.
- `tests/pgwire.rs`: ready-for-query transaction states after real protocol support lands.

## Exit Gate

This sprint is complete when transaction/session semantics are covered by validator-clean tests, targeted transaction tests pass, `cargo build` passes, and Clippy is clean with warnings denied.

