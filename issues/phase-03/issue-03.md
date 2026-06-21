# Phase 03 Issue 03: Index Performance Feedback

Milestone: Read-Model Performance
Area: Planner Intelligence
Status: Open
Priority: P2

## Requirements

Track observed index selectivity and cost so the planner can choose among competing indexes more accurately.

## Functional Scope

- Record per-index feedback for predicate shape, estimated rows, scanned index entries, fetched row blobs, returned rows, elapsed time, and fallback reason.
- Partition feedback by collection, index id/version, schema epoch, predicate shape, and database.
- Use feedback in cost-informed planning to prefer indexes that have lower observed cost for matching shapes.
- Invalidate feedback when index definition, schema epoch, collection, or database changes.
- Expose per-index feedback through metrics and EXPLAIN diagnostics without leaking bind values.

## Non-Goals

- Do not change row blob truth or index correctness semantics.
- Do not implement automatic index creation/drop decisions here.

## Acceptance Criteria

- Index feedback is captured after indexed query execution and ignored for full-scan fallback.
- Competing index selection can change when feedback consistently favors one index.
- Stale or missing feedback falls back to static estimates.
- Metrics show feedback reads, writes, invalidations, and selected index influence.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback capture, competing index choice, stale invalidation, missing fallback, privacy/no bind values, and EXPLAIN diagnostics.
- Include planner and metrics tests.

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
- `cargo test --locked --test planner_estimates --test planner_indexes --test planner_physical`
- `cargo test --locked --test metrics_feedback --test metrics_runtime --test metrics_search --test metrics_plan_pgwire`
- `cargo test --locked --test plan_cache --test integration_sql_ordering --test integration_sql_projection`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
