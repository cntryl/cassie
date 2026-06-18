# Sprint 33 - Compatibility Matrix and CI Gate

Previous: [Sprint 32 - Extended Query Portals and Recovery](sprint-32.md)
Next: [Sprint 34 - REST, Operations, Packaging, and V1 Release Gate](../sprint-34.md)

## Goal

Turn Cassie's PostgreSQL compatibility promise into a versioned, repeatable tokio-postgres release gate.

## Requirements

- Define the V1 compatibility matrix around the official Rust driver (`tokio-postgres`) first.
- Cover mandatory fixtures for connect/auth, catalog metadata, simple query, prepared query, parameterized DDL/DML, user-defined functions, procedures, recursive CTEs, and error-recovery.
- Keep prepared statements and portals session-local while validating the shared server-side plan and execution path.
- Verify result-format handling through the driver, including binary result frames on prepared statements.
- Add optional external-client fixtures for `psql`, libpq, a second language driver, and a BI/metadata probe when the tools are available locally or in CI.
- Document view coverage explicitly as deferred until native SQL `CREATE VIEW` / `DROP VIEW` support exists; do not leave the fixture unmentioned.
- Add CI commands or workflow steps that run the compatibility gate consistently.

## Acceptance Criteria

- The compatibility matrix is documented, versioned, and tied to this sprint.
- Mandatory tokio-postgres fixtures pass locally.
- Optional fixtures skip clearly when prerequisites are unavailable.
- Failures identify protocol, catalog, SQL, auth, or type-system causes instead of hanging or silently degrading.
- The compatibility gate is runnable on demand from CI or a single documented command.
- `cargo test --test compatibility_matrix`, `cargo build`, Clippy, and touched-test validation pass.

## Tests

- `tests/compatibility_matrix.rs` for tokio-postgres connect, metadata, prepared query, DDL/DML, function, procedure, recursive CTE, and recovery coverage.
- `tests/pgwire_extended_query.rs` for low-level wire sequencing, startup readiness, `Describe` metadata, and recovery behavior.
- Documentation checks for the supported-client matrix and the explicitly deferred fixtures.

## Exit Gate

This sprint is complete when the tokio-postgres compatibility matrix is green, the matrix is documented, skipped fixtures are explicit, and the release gate can be run repeatedly without manual cleanup.
