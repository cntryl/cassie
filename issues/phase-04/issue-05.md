# Phase 04 Issue 05: Runtime Boundary Diagnostics

Milestone: Runtime Boundary Discipline
Area: Observability
Status: Open
Priority: P2

## Requirements

Make runtime boundary behavior visible enough to detect regressions.
Cassie should be able to explain whether pgwire and REST requests used explicit blocking boundaries, whether blocking work failed to join, and whether transport tasks are accumulating degraded boundary behavior.

## Dependencies

- Depends on phase 04 issue 01 for contract vocabulary.
- Depends on phase 04 issue 02 for stable blocking operation names.
- Consumes helper patterns from phase 04 issues 03 and 04.

## Handoff

- Provides runtime-boundary observability consumed by static audit, benchmarks, and production diagnostics.

## Functional Scope

- Add runtime counters or structured diagnostics for blocking-boundary entry, completion, error, join failure, and elapsed time.
- Distinguish pgwire simple query, pgwire extended describe, pgwire extended execute, pgwire auth, REST route, REST auth, and REST embedding/search boundaries.
- Expose diagnostics through existing metrics/admin surfaces where appropriate.
- Avoid high-cardinality route, SQL, bind value, credential, or document payload labels.

## Implementation Plan

### Step 1: Define diagnostic vocabulary

- Extend runtime docs or performance contracts with boundary event names.
- Keep labels stable and low-cardinality: interface, operation, outcome, and elapsed bucket if buckets already exist.

### Step 2: Add runtime state

- Add counters to the existing runtime metrics state instead of creating a second metrics subsystem.
- Include at minimum:
  - blocking boundary started
  - blocking boundary completed
  - blocking boundary errored
  - blocking boundary join failed
  - blocking boundary elapsed total
- Add methods that helper modules can call without exposing runtime internals broadly.

### Step 3: Wire helpers

- Record pgwire and REST boundary events at helper ownership points.
- Ensure each blocking task records exactly one terminal outcome.
- Ensure join failures are visible and mapped into existing error handling.

### Step 4: Expose diagnostics

- Add metrics snapshot fields or REST metrics JSON fields consistent with current metrics conventions.
- Keep existing metrics field names stable.
- Do not include SQL text, route path parameters, credentials, or request bodies.

### Step 5: Add tests

- Add focused runtime metrics tests for success, application error, and join failure where feasible.
- Add pgwire or REST integration assertions only if the metrics surface is already used there.

## Non-Goals

- Do not add a new metrics backend.
- Do not expose high-cardinality or sensitive labels.
- Do not tune Tokio runtime settings in this issue.

## Acceptance Criteria

- Boundary usage is visible through runtime diagnostics.
- Pgwire and REST helper paths record success and failure outcomes.
- Join failures are observable and do not panic.
- Metrics remain low-cardinality and avoid sensitive payloads.

## Required Tests

- Runtime metrics tests for boundary counters.
- Pgwire or REST metrics tests only where the changed surface requires it.
- `cntryl-tools validate-tests -f <path>` for every touched test file.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update phase 04 static-audit issue if diagnostics change helper boundaries.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
