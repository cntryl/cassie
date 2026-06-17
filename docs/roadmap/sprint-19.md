# Sprint 19 - Compatibility Matrix and CI Gate

Previous: [Sprint 18 - Auth, Roles, and Security Posture](sprint-18.md)  
Next: [Sprint 20 - Real PostgreSQL Wire Protocol Core](sprint-20.md)

## Goal

Turn Cassie's PostgreSQL compatibility promise into a concrete, repeatable compatibility matrix with automated smoke tests for clients, drivers, ORMs, migration workflows, and BI-style metadata probes.

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

- Define the V1 client compatibility matrix for `psql`, libpq, one Rust driver, one Python driver, one Node driver, one ORM, one migration tool, and one BI-style metadata probe.
- Add compatibility fixtures that run representative connect, metadata, simple query, prepared query, DDL, DML, search, vector, and error-recovery flows.
- Make optional external-client tests skippable when tools are unavailable, while keeping core protocol fixtures mandatory.
- Document exact supported and unsupported PostgreSQL behavior observed by the matrix.
- Ensure matrix failures are actionable and map to a sprint-owned behavior area.
- Add CI commands or scripts that run the compatibility gate consistently.

## Acceptance Criteria

- Compatibility matrix is documented and versioned.
- Mandatory compatibility fixtures pass in local test runs.
- Optional external-client fixtures skip clearly when prerequisites are unavailable.
- At least one driver, one ORM or migration workflow, and one metadata probe are represented.
- Failures include enough context to identify protocol, catalog, SQL, auth, or type-system causes.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/compatibility_matrix.rs`: core compatibility matrix metadata and mandatory fixture coverage.
- `tests/pgwire.rs`: protocol-level client behavior fixtures.
- External smoke fixtures for selected clients where available.
- CI/documentation checks that list supported clients and skipped optional fixtures clearly.

## Exit Gate

This sprint is complete when compatibility expectations are automated, documented, validator-clean, and ready to run as a release gate alongside `cargo build`, Clippy, and targeted tests.

