# Sprint 34 - REST, Operations, Packaging, and V1 Release Gate

Previous: [Sprint 33 - Compatibility Matrix and CI Gate](sprint-33.md)
Next: [Roadmap README](README.md)

## Goal

Finish the secondary REST/admin surface, operational readiness, single-container packaging, and full V1 acceptance suite.

## Requirements

- Complete Hyper REST endpoints for collections, documents, indexes/search, health, and metrics.
- Ensure REST uses shared Cassie runtime, catalog, validation, and Midge-backed row persistence paths.
- Map REST validation, storage, embedding, and unsupported errors to deterministic HTTP status codes and JSON payloads.
- Add single-container startup, readiness, liveness, graceful shutdown, and restart behavior.
- Keep packaging compatible with existing Docker and compose workflow.
- Add end-to-end V1 acceptance coverage for Midge, SQL, full-text search, vector, hybrid, REST, pgwire, restart, and error paths.

## Acceptance Criteria

- Required REST endpoints pass happy-path and bad-input tests.
- Health and metrics expose useful operational state.
- Single-container deployment starts, becomes ready, shuts down cleanly, and restarts with hydrated catalog.
- V1 acceptance confirms document CRUD, SQL execution, full-text search, vector search, hybrid search, and PostgreSQL wire client compatibility.
- Full `cargo test`, `cargo build`, Clippy, and touched-test validation pass.

## Tests

- REST tests for collection/document CRUD, bad payloads, vector dimension validation, and metrics.
- End-to-end SQL, REST, pgwire, search, vector, hybrid, restart, and error-path acceptance tests.
- Single-container startup/shutdown smoke test.

## Exit Gate

This sprint is complete when V1 acceptance criteria are green, PostgreSQL practical client compatibility is proven, packaging is ready, full `cargo test` passes, `cargo build` passes, and Clippy is clean with warnings denied.
