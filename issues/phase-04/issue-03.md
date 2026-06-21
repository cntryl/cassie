# Phase 04 Issue 03: Pgwire Blocking Boundary

Milestone: Runtime Boundary Discipline
Area: Pgwire
Status: Open
Priority: P2

## Requirements

Move synchronous pgwire engine work behind explicit blocking boundaries while preserving PostgreSQL wire behavior.
Pgwire connection tasks should await socket IO and frame writes, but query execution, describe planning, extended execution, and authentication must not run directly on Tokio IO workers.

## Dependencies

- Depends on phase 04 issue 01 for the boundary contract.
- Depends on phase 04 issue 02 for blocking operation names and auth/embedding classification.

## Handoff

- Provides the pgwire runtime boundary consumed by diagnostics and static-audit work.

## Functional Scope

- Offload simple-query execution entered from `src/pgwire/connection.rs`.
- Offload extended-query describe and execute paths.
- Offload password authentication when a pgwire connection enters `AwaitPassword`.
- Preserve existing prepared statement, portal, SQLSTATE, ready-for-query, transaction-status, limit, and error behavior.
- Preserve synchronous `Cassie` engine APIs.

## Implementation Plan

### Step 1: Add failing pgwire coverage

- Add or extend pgwire tests to cover simple query, extended prepare/bind/execute, describe, auth success, and auth failure after the boundary helper is introduced.
- Keep async tests on a current-thread Tokio runtime, following repo rules.
- Add a boundary-oriented assertion only if diagnostics from issue 05 already exist; otherwise keep behavior coverage here.

### Step 2: Introduce a pgwire blocking helper

- Add a small pgwire-owned helper module or function that wraps `tokio::task::spawn_blocking`.
- Use operation names from issue 02: `pgwire_auth`, `pgwire_simple_query`, `pgwire_describe`, and `pgwire_execute`.
- The helper must map join errors into `CassieError::Execution` or an equivalent pgwire error path without panicking.
- Keep cloned data explicit: clone `Arc<Cassie>`, `CassieSession`, parsed statements, fingerprints, params, and portal result formats before offload.

### Step 3: Move simple query execution

- Replace direct `cassie.execute_sql(session, &sql, Vec::new())` calls from async connection code with the helper.
- Preserve existing write order: result frames or error response first, then `ReadyForQuery`.
- Ensure transaction failure marking remains owned by existing app execution logic.

### Step 4: Move extended describe and execute

- Offload `describe_parsed_statement` from describe handling.
- Offload `execute_preparsed_statement_with_mode` from execute handling.
- Apply portal row limit after the blocking result returns, matching current behavior.
- Keep row writes async and sequential on the connection task.

### Step 5: Move password authentication

- Offload `authenticate_role` from `AwaitPassword`.
- Preserve auth success/failure metrics and fatal error mapping.
- Keep startup packet parsing and protocol validation async/sync inline only where it is CPU-trivial.

## Non-Goals

- Do not make `Cassie::execute_sql`, `describe_parsed_statement`, or auth APIs async.
- Do not change pgwire frame parsing, frame encoding, prepared-statement storage, or portal semantics.
- Do not add concurrent execution within one pgwire connection.

## Acceptance Criteria

- Pgwire async connection code no longer directly runs blocking query, describe, execute, or password-auth work.
- Existing pgwire behavior and error mapping are preserved.
- Blocking join failures are converted to protocol errors without panics.
- Tests cover simple query, extended query, describe, auth success, and auth failure.

## Required Tests

- `tests/pgwire_simple_query.rs`
- `tests/pgwire_extended_prepared.rs`
- `tests/pgwire_startup.rs`
- Any touched pgwire metrics test

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
