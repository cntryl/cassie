# Sprint 20 - Real PostgreSQL Wire Protocol Core

Previous: [Sprint 19 - Compatibility Matrix and CI Gate](sprint-19.md)  
Next: [Sprint 21 - PostgreSQL Extended Query Protocol and Client Compatibility](sprint-21.md)

## Goal

Replace the current simplified line-oriented pgwire path with real PostgreSQL frontend/backend binary message framing for practical client compatibility.

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

- Implement PostgreSQL frontend/backend binary message framing.
- Implement startup packet parsing.
- Define and implement SSL request behavior for V1.
- Implement authentication negotiation compatible with practical clients.
- Implement simple query protocol.
- Emit PostgreSQL-compatible row description, data row, command complete, error response, and ready-for-query messages.
- Map Cassie scalar types to PostgreSQL OIDs and text/binary format policies.
- Preserve deterministic ready-state transitions.
- Retire or convert existing simplified protocol tests.
- Keep all simple-query execution routed through `Cassie::execute_sql`.

## Acceptance Criteria

- `psql` can connect, authenticate, run simple `SELECT`, and receive rows.
- A libpq-style client smoke test passes.
- Unsupported startup options fail with PostgreSQL-style errors without crashing the session.
- Ready-for-query is emitted where practical clients expect it.
- Existing simplified protocol tests are retired or converted to real wire tests.
- Simple-query wire execution returns the same rows as direct SQL execution.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/pgwire.rs`: binary startup packet parse behavior.
- `tests/pgwire.rs`: authentication success and failure behavior.
- `tests/pgwire.rs`: simple query lifecycle with row metadata and data rows.
- `tests/pgwire.rs`: unsupported startup or message behavior returns PostgreSQL-style error response.
- Add a real client smoke test for `psql` or libpq when available in the environment.

## Exit Gate

This sprint is complete when the real binary wire path supports startup, auth, simple query, row output, errors, ready-for-query, practical `psql` or libpq smoke compatibility, validator-clean tests, `cargo build`, and Clippy with warnings denied. When the exit gates are green, move this file from `docs/roadmap/sprint-20.md` to `docs/roadmap/completed/sprint-20.md` and update the roadmap links to point at the completed copy.
