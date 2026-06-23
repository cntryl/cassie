# Phase 09 Issue 07: Pgwire Client Probe Expansion

Milestone: Production Depth And Operational Orchestration
Area: PostgreSQL Compatibility
Status: Open
Priority: P1

## Goal

Expand client compatibility probes beyond the default tokio-postgres baseline without making brittle optional dependencies part of the normal test suite.

## Dependencies

- Phase 08 client matrix baseline is complete.
- `docs/postgres-compatibility.md` marks tokio-postgres supported baseline and other clients as planned or experimental.

## Requirements

- Add opt-in probes for at least one non-tokio client workflow per slice, starting with the lowest-friction local dependency.
- Keep default `cargo test --locked` deterministic and service-free.
- Document install/env requirements for optional probes.
- Cover read-model workflows: connect, catalog probe, simple query, prepared query, DDL/DML smoke, error handling.
- Update the client matrix without implying full PostgreSQL or ORM parity.

## Acceptance Criteria

- The selected client has an ignored or feature-gated probe that can run locally when dependencies are installed.
- The default suite remains green without the optional client.
- Compatibility docs distinguish supported, experimental, planned, and unsupported workflows.
- SQLSTATE and catalog behavior gaps found by the probe are either fixed or tracked.

## Implementation Plan

1. Pick one client from the matrix based on local feasibility and dependency isolation.
2. Write an ignored/fenced test that skips unless explicit env vars are set.
3. Implement only compatibility fixes needed for the read-model workflow and supported by existing boundaries.
4. Update `docs/postgres-compatibility.md` and production-readiness evidence.
5. Leave remaining clients marked planned or experimental.

## Required Tests

- Default `cargo test --locked` must pass with the optional probe ignored.
- Optional probe command documented in the test and docs.
- `cntryl-tools validate-tests -f tests/compatibility_matrix.rs` or the new client-specific test file.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm no optional dependency is required for the default suite.
- Confirm unsupported ORM/migration behavior remains explicit.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
