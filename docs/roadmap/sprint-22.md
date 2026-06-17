# Sprint 22 - REST, Operations, Packaging, and V1 Release Gate

Previous: [Sprint 21 - PostgreSQL Extended Query Protocol and Client Compatibility](sprint-21.md)  
Next: [Roadmap README](README.md)

## Goal

Finish the secondary REST/admin surface, operational readiness, single-container packaging, and full V1 acceptance suite.

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

- Complete Hyper REST endpoints for collections, documents, indexes/search, health, and metrics.
- Ensure REST uses shared `Cassie` runtime, catalog, validation, and Midge-backed persistence paths.
- Map REST validation, storage, embedding, and unsupported errors to deterministic HTTP status codes and JSON payloads.
- Add single-container startup, readiness, liveness, graceful shutdown, and restart behavior.
- Keep packaging compatible with the existing Docker and compose workflow.
- Add end-to-end V1 acceptance coverage for Midge, SQL, full-text search, vector, hybrid, REST, pgwire, restart, and error paths.
- Run final release gates from a clean checkout state where possible.

## Acceptance Criteria

- Required REST endpoints pass happy-path and bad-input tests.
- Health and metrics expose useful operational state.
- Single-container deployment starts, becomes ready, shuts down cleanly, and restarts with hydrated catalog.
- Full `cargo test` passes.
- V1 acceptance confirms document CRUD, SQL execution, full-text search, vector search, hybrid search, and PostgreSQL wire client compatibility.
- No manual setup is required beyond documented environment variables and Midge data path configuration.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/rest.rs`: collection and document CRUD, bad payloads, and vector dimension validation.
- `tests/rest_embeddings.rs`: REST ingest and vector search paths.
- End-to-end SQL, REST, pgwire, search, vector, hybrid, and restart acceptance tests.
- Single-container startup/shutdown smoke test.
- Final full `cargo test`.
- Validate every touched test file with `cntryl-tools`.

## Exit Gate

This sprint is complete when all V1 acceptance criteria are green, PostgreSQL practical client compatibility is proven, REST and operational surfaces are stable, single-container deployment is ready to ship, `cargo build` passes, `cargo clippy --all-targets --all-features -- -D warnings` passes, and full `cargo test` passes.
