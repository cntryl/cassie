# Phase 04 Issue 01: Runtime Boundary Contracts

Milestone: Runtime Boundary Discipline
Area: Contracts
Status: Open
Priority: P2

## Requirements

Define Cassie's async/sync boundary contract before changing runtime behavior.
Phase 04 must protect the Rust design choice that async belongs at transport and coordination edges, while the database engine remains synchronous unless a real async dependency creates an explicit separate API.

Correctness is required but not sufficient: async entrypoints must prove that blocking work is not hidden on Tokio IO workers.

## Dependencies

- Depends on `docs/performance-contracts.md` for contract and assertion vocabulary.
- Depends on current pgwire and REST async entrypoints as the first boundary inventory.

## Handoff

- Provides the runtime-boundary vocabulary consumed by the rest of phase 04.

## Functional Scope

- Define async entrypoints: `main`, pgwire listener/connection, REST listener/router, shutdown signal handling, socket frame IO, and HTTP body collection.
- Define synchronous engine ownership: query execution, parser, binder, planner, executor, catalog, Midge adapter, auth role lookup/password verification, REST document/index/search handlers, and embedding providers.
- Define required blocking-boundary behavior for sync work entered from async transport tasks.
- Define forbidden behavior: direct query/auth/embedding/storage work inside async IO tasks, async trait migration without a real async dependency, and unbounded detached blocking work.
- Define the initial diagnostic and static-audit expectations for later issues.

## Implementation Plan

### Step 1: Inventory current boundaries

- Read `src/main.rs`, `src/pgwire/server.rs`, `src/pgwire/connection.rs`, and `src/rest/router.rs`.
- Document every async function and every synchronous Cassie call made from it.
- Read `src/app/query.rs`, `src/app/roles.rs`, `src/app/documents.rs`, `src/rest/*`, and `src/embeddings/provider.rs` to identify sync engine/auth/provider ownership.

### Step 2: Add boundary contract docs

- Add a runtime-boundary section to `docs/performance-contracts.md`.
- Define terms: async transport task, synchronous engine call, blocking boundary, blocking pool, boundary timeout, degraded boundary, and direct-blocking violation.
- State that SQL, REST, and pgwire are interfaces, not the engine execution model.

### Step 3: Classify supported boundaries

- Create a table for pgwire simple query, pgwire extended query, pgwire describe, REST route handlers, REST authentication, embedding-backed vector search, and shutdown.
- For each boundary, name the async owner, sync owner, required offload behavior, forbidden inline behavior, and expected error mapping.

### Step 4: Define validation ownership

- Map pgwire behavior to `tests/pgwire_simple_query.rs`, `tests/pgwire_extended_prepared.rs`, `tests/pgwire_startup.rs`, and metrics tests.
- Map REST behavior to existing focused REST tests.
- Map static audits to a focused test or script-backed test that searches async transport modules for forbidden direct calls.

### Step 5: Close the contract issue

- Do not refactor pgwire or REST behavior in this issue unless a tiny test helper is needed to make the contract measurable.
- Update later phase 04 issues if the contract names different boundaries than expected.

## Non-Goals

- Do not convert engine APIs to async.
- Do not change query, REST, pgwire, auth, or embedding semantics in this issue.
- Do not tune Tokio worker counts or blocking-thread counts before the boundary contract exists.

## Acceptance Criteria

- Runtime-boundary terminology is documented.
- Every current async transport entrypoint is classified.
- Every current synchronous engine/auth/provider call from async transport has an intended boundary owner.
- Forbidden direct-blocking behavior is explicit enough to test or audit.
- Later phase 04 issues can implement without redefining the boundary policy.

## Required Tests

- Add docs/static-audit support only where needed.
- If reusable fixture code is added, include deterministic fixture tests in `should_` style.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update roadmap/docs references when the contract surface changes.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
