# Sprint 33 - Compatibility Matrix and CI Gate

Previous: [Sprint 32 - Extended Query Portals and Recovery](completed/sprint-32.md)
Next: [Sprint 34 - REST, Operations, Packaging, and V1 Release Gate](sprint-34.md)

## Goal

Turn Cassie's PostgreSQL compatibility promise into repeatable smoke tests and documentation after real wire support exists.

## Requirements

- Define the V1 compatibility matrix for `psql`, libpq, one Rust driver, one Python or Node driver, one ORM or migration tool, and one BI-style metadata probe.
- Add fixtures for connect, metadata, simple query, prepared query, DDL, DML, search, vector, and error-recovery flows.
- Make optional external-client tests skip clearly when tools are unavailable.
- Add CI commands or scripts that run the compatibility gate consistently.

## Acceptance Criteria

- Compatibility matrix is documented and versioned.
- Mandatory fixtures pass locally.
- Optional fixtures skip clearly when prerequisites are unavailable.
- Failures identify protocol, catalog, SQL, auth, or type-system causes.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- `tests/compatibility_matrix.rs` for matrix metadata and mandatory fixture coverage.
- Pgwire/client smoke tests for selected practical clients.
- Documentation checks for supported clients and skipped optional fixtures.

## Exit Gate

This sprint is complete when compatibility expectations are automated, documented, validator-clean, and ready to run as a release gate.
