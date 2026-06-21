# Phase 04 Issue 02: Auth And Embedding Blocking Discipline

Milestone: Runtime Boundary Discipline
Area: Auth and Embeddings
Status: Open
Priority: P2

## Requirements

Make expensive synchronous auth and embedding behavior explicit at async entrypoints.
Argon2 password verification, role lookup through storage, blocking provider HTTP calls, and retry sleeps must remain synchronous engine/provider work, but async transports must enter them only through documented blocking boundaries.

## Dependencies

- Depends on phase 04 issue 01 for boundary definitions.

## Handoff

- Provides auth, embedding, and expensive-engine operation classifications consumed by pgwire, REST, diagnostics, and static-audit work.

## Functional Scope

- Classify `authenticate_role`, `lookup_role`, `verify_password`, and embedding provider calls as synchronous work.
- Classify query execution, describe planning, REST handler calls, storage access, and embedding-backed vector search as synchronous work when entered from async transport.
- Define the operation names transport helpers must use: `pgwire_auth`, `pgwire_simple_query`, `pgwire_describe`, `pgwire_execute`, `rest_auth`, `rest_route`, and `rest_embedding_search`.
- State exactly which synchronous calls remain allowed inline because they are CPU-trivial parsing/validation and cannot block on IO, hash work, storage, or provider HTTP.
- Preserve provider trait shape: `EmbeddingProvider` remains synchronous.
- Document retry sleep behavior and timeout ownership for blocking providers.

## Implementation Plan

### Step 1: Inventory auth and embedding call paths

- Read `src/app/roles.rs`, `src/app/auth.rs`, `src/app/vector_search.rs`, `src/app/documents.rs`, and `src/embeddings/*/provider.rs`.
- Identify which calls can execute Argon2, storage role lookup, blocking HTTP, or `std::thread::sleep`.
- Map each call path back to pgwire, REST, tests, benchmarks, or synchronous direct app usage.

### Step 2: Define blocking operation taxonomy

- Add the taxonomy to the runtime-boundary contract docs from issue 01.
- For each operation name, define async owner, sync owner, inputs to clone before offload, expected error mapping, and whether cancellation waits for completion.
- Keep operation names stable for diagnostics and static audit.

### Step 3: Add behavior fixtures where risk is known

- Add auth success/failure tests for pgwire and REST only where existing coverage does not lock behavior.
- Add embedding unavailable/invalid response tests for REST vector search if current coverage does not lock error mapping.
- Do not add transport helpers in this issue; issues 03 and 04 own helper implementation.

### Step 4: Document provider behavior

- Add a short note to the runtime-boundary docs that current providers use blocking clients and retry sleeps.
- State that an async provider API would be a future additive interface, not a replacement required by phase 04.

## Non-Goals

- Do not replace Argon2, provider HTTP clients, or retry policy.
- Do not convert `EmbeddingProvider` to an async trait.
- Do not alter password, role, vector-search, or embedding validation semantics.

## Acceptance Criteria

- Auth, embedding, query, describe, REST handler, and storage work entered from async transports is classified before transport helpers are implemented.
- Synchronous direct app use remains supported.
- Provider retry and timeout behavior is documented as blocking work.
- Pgwire and REST implementation issues have stable operation names, clone requirements, and error-mapping rules to implement mechanically.

## Required Tests

- Pgwire auth tests when pgwire auth code is touched.
- REST auth/search/embedding tests when REST auth or vector search code is touched.
- `cntryl-tools validate-tests -f <path>` for every touched test file.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update phase 04 diagnostics/static-audit issues if auth or embedding boundary ownership changed.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
