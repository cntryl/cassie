# Phase 04 Issue 06: Boundary Regression Tests And Static Audit

Milestone: Runtime Boundary Discipline
Area: Testing
Status: Open
Priority: P2

## Requirements

Add regression coverage that prevents async/sync boundary drift.
Future changes should fail loudly when async transport modules directly call synchronous engine/auth/provider work that should go through an explicit blocking boundary.

## Dependencies

- Depends on phase 04 issue 01 for forbidden direct-blocking behavior.
- Depends on phase 04 issue 02 for the protected synchronous API list and operation names.
- Consumes final helper names and diagnostics from phase 04 issues 03 through 05.

## Handoff

- Provides the enforcement layer required to keep phase 04 from regressing after implementation.

## Functional Scope

- Add static-audit coverage for async transport modules.
- Add behavioral tests that prove pgwire and REST still preserve existing semantics after boundary helpers are used.
- Add metrics assertions from issue 05 where available.
- Keep audits narrow enough to avoid blocking valid synchronous app usage outside async transport modules.

## Implementation Plan

### Step 1: Define audit rules

- Audit only transport-owned async modules first: pgwire connection/server and REST router.
- Forbid direct calls from those modules to known blocking APIs when the call is not inside an approved boundary helper.
- Initial forbidden call patterns should cover query execution, describe, preparsed execution, auth, REST handler calls, vector search, and direct embedding provider calls.

### Step 2: Add static audit test

- Add a focused test file or module that reads source files and asserts forbidden patterns are absent outside approved helper scopes.
- Keep the audit explicit and maintainable rather than a broad lint framework.
- Include failure messages that name the expected helper.

### Step 3: Add behavior regression tests

- Preserve pgwire simple query, extended query, describe, auth success, and auth failure behavior.
- Preserve REST health, metrics, collection/document/index/search, auth success, auth failure, and forbidden behavior.
- Add diagnostics assertions only where issue 05 has exposed stable metrics.

### Step 4: Add close-out audit commands

- Add issue close-out instructions for `rg` checks over async modules.
- Ensure phase 00 index checks still pass after issue completion/deletion.

## Non-Goals

- Do not build a general Rust linter.
- Do not forbid synchronous Cassie APIs in synchronous tests, benchmarks, embedded usage, or engine modules.
- Do not audit every possible CPU-heavy expression; focus on known boundary-crossing APIs.

## Acceptance Criteria

- Static audit fails if pgwire or REST async modules call protected synchronous APIs directly.
- Behavior tests prove protocol and HTTP semantics remain stable.
- Diagnostics tests prove boundary metrics when issue 05 has exposed them.
- Audit rules are documented and narrow enough to avoid false positives in unrelated code.

## Required Tests

- Static audit test in a focused test file.
- Relevant pgwire and REST behavior tests touched by the boundary implementation.
- Runtime metrics tests when diagnostics are part of the acceptance surface.
- `cntryl-tools validate-tests -f <path>` for every touched test file.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update docs if protected API names or helper names changed.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
