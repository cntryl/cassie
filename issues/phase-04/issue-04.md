# Phase 04 Issue 04: REST Blocking Boundary

Milestone: Runtime Boundary Discipline
Area: REST
Status: Open
Priority: P2

## Requirements

Move synchronous REST handler work behind explicit blocking boundaries while preserving HTTP behavior.
REST listener and route code should await connection IO and body collection, but collection, document, index, search, auth, storage, and embedding-backed operations must not run directly on Tokio IO workers.

## Dependencies

- Depends on phase 04 issue 01 for the boundary contract.
- Depends on phase 04 issue 02 for blocking operation names and auth/embedding classification.
- Should align join-error mapping and diagnostics vocabulary with phase 04 issue 03.

## Handoff

- Provides the REST runtime boundary consumed by diagnostics and static-audit work.

## Functional Scope

- Keep HTTP accept, connection serving, request routing, and body collection async.
- Offload non-public route handlers that call sync Cassie, Midge, catalog, index, search, or embedding code.
- Offload REST authentication when password verification or role lookup is required.
- Preserve public health, liveness, and metrics route behavior unless the contract requires offload.
- Preserve HTTP status mapping and JSON response shape.

## Implementation Plan

### Step 1: Add failing REST coverage

- Extend focused REST tests for collection create/list, document create/get/delete, index create, vector search, auth success, auth failure, and forbidden non-admin role where existing tests do not already cover them.
- Keep test files subsystem-specific and split before approaching 1,000 lines.

### Step 2: Introduce a REST blocking helper

- Add a small REST-owned helper that wraps `tokio::task::spawn_blocking`.
- Use operation names from issue 02: `rest_auth`, `rest_route`, and `rest_embedding_search`.
- The helper must return `Result<T, CassieError>` or route-mapped errors without losing existing status-code mapping.
- Keep body bytes, method/path strings, collection names, ids, and `Arc<Cassie>` clones explicit before offload.

### Step 3: Split route IO from sync work

- Keep method/path parsing and body collection in async route code.
- Move route-specific calls such as `collections::create`, `documents::create`, `indexes::create`, `search::vector_search`, document get, and delete into the blocking helper.
- Continue recording REST request metrics exactly once per request.

### Step 4: Move REST authentication

- Parse authorization headers in async route code.
- Offload `authenticate_role` and `lookup_role` through the helper when auth is required.
- Preserve unauthorized, forbidden, and service-unavailable mapping.

### Step 5: Preserve shutdown behavior

- Ensure listener shutdown still stops new accepts.
- Document whether in-flight blocking route work is allowed to finish before the connection future completes.

## Non-Goals

- Do not make REST handler modules async.
- Do not change route paths, methods, status codes, or response JSON.
- Do not make embedding providers async in this issue.

## Acceptance Criteria

- REST async route code no longer directly runs blocking handler/auth/search work.
- Public health/liveness/metrics routes remain lightweight and correct.
- Existing HTTP status and JSON response behavior are preserved.
- In-flight blocking route behavior during shutdown is documented.

## Required Tests

- Existing focused REST tests for documents, indexes, search, embeddings, auth, health, and metrics.
- Add `should_` tests only in subsystem-specific files.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update phase 04 diagnostics/static-audit issues if helper names or boundary ownership changed.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
