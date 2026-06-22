# Phase 06 Issue 02: Keyset Pagination

Milestone: Read-Model Read Optimization
Area: Executor
Status: Open
Priority: P2

## Requirements

Prefer seek/keyset pagination and bounded continuation scans for interactive read-model pages instead of offset-driven scans that discard work.

## Dependencies

- Depends on the archived phase 04 read access-path contract surface in `docs/performance-contracts.md` and `issues/phase-04/README.md` for the ordered-page and filtered-page contracts.
- Depends on phase 06 issue 01 for bounded ordered scan planning.

## Handoff

- Provides the pagination model used by latency-sensitive read-model list views.

## Functional Scope

- Add keyset/seek pagination support for supported ordered-page patterns.
- Keep offset pagination correct but explicitly degraded when it cannot use an efficient path.
- Expose continuation key semantics through SQL-visible or Cassie-specific documented surfaces where needed.
- Preserve deterministic ordering and tie handling.

## Required Access Path

- Query proves a stable ordering over indexed or projection-shaped keys.
- Cursor predicate is represented as a bounded seek/range start, not as rows to discard.
- Executor streams from the seek point and stops at page size.
- EXPLAIN distinguishes `keyset` from degraded `offset` pagination.

## Forbidden Access Path

- Scanning from the beginning of a collection and discarding prior pages for keyset-eligible queries.
- Keyset labels on unordered or non-deterministically ordered results.
- Changing PostgreSQL `OFFSET/LIMIT` result semantics.
- Returning unstable page ordering when sort keys tie.

## Implementation Plan

### Step 1: Start with canonical SQL shape, no new syntax

- Do not add new cursor syntax in the first implementation.
- Recognize canonical keyset SQL patterns users can already write, such as:
  - `WHERE created_at < $cursor ORDER BY created_at DESC, id ASC LIMIT 50`
  - `WHERE tenant_id = $tenant AND created_at < $cursor ORDER BY created_at DESC, id ASC LIMIT 50`
  - tie-safe variants with `created_at = $cursor AND id > $last_id`
- Document any Cassie-specific cursor helper as a future extension, not required for this issue.

### Step 2: Add failing planner and EXPLAIN tests

- Add planner tests in `tests/planner_physical.rs` or `tests/planner_indexes.rs`:
  - `should_mark_descending_time_cursor_as_keyset_scan`
  - `should_require_tie_breaker_for_stable_keyset_order`
  - `should_fallback_for_offset_without_cursor_predicate`
- Add EXPLAIN tests in `tests/integration_sql_explain.rs`:
  - `keyset_pagination=true`
  - `pagination=offset_degraded` for deep offset fallback
  - `cursor_bound=...` or equivalent diagnostic once implemented.

### Step 3: Extend physical plan metadata

- Add `pagination_strategy` or reuse an access-path enum from issue 01 with values like `none`, `offset`, `keyset`, and `degraded_offset`.
- Add optional `cursor_bound` or `seek_bound` metadata that is safe to expose without leaking bind values.
- Keep `scan_limit` behavior for ordinary `LIMIT/OFFSET`; keyset should use a limit of page size, not offset plus page size.

### Step 4: Add seek-bound extraction

- In `src/planner/physical.rs`, add helper functions to prove:
  - order columns are deterministic
  - cursor predicates reference the leading order key and optional tie key
  - predicates are compatible with ASC/DESC direction
  - tenant/equality prefix predicates are preserved ahead of the cursor range
- Start with simple column/literal/param predicates.

### Step 5: Wire executor and storage

- Extend the scan request from issue 01 with an optional seek/range start.
- Update `src/executor/execution/projected_read.rs` and `src/executor/scan.rs` to pass the keyset bound to Midge only when the planner proves it.
- Add Midge range/prefix scan support only for key layouts that can honor ordering and bounds.
- If storage cannot yet honor the bound, return explicit fallback and keep result correctness.

### Step 6: Preserve offset compatibility

- Leave `OFFSET/LIMIT` behavior intact.
- Add diagnostics that offset pagination is degraded when no matching keyset predicate exists.
- Do not rewrite arbitrary offset queries into keyset behavior.

### Step 7: Benchmark validation

- Add a focused ordered-page benchmark to `tier2_subsystem_executor` or `tier3_system_query` after keyset execution exists.
- Include a deep-offset degraded benchmark only if needed to document the performance gap.

## Non-Goals

- Do not break PostgreSQL-compatible OFFSET/LIMIT semantics.
- Do not claim keyset support for unordered result sets.

## Acceptance Criteria

- Supported ordered-page patterns can stop after page-size work rather than scanning discarded offsets.
- Offset-based paths remain correct and visibly degraded where appropriate.
- Benchmarks demonstrate bounded continuation behavior.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering forward continuation, tie behavior, degraded offset fallback, and EXPLAIN/diagnostic output.
- Include planner and integration coverage.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test integration_sql_ordering --test integration_sql_predicates --test integration_sql_explain`
- `cargo test --locked --test planner_indexes --test planner_physical`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_executor --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
