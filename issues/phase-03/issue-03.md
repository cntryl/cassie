# Phase 03 Issue 03: Index Performance Feedback

Milestone: Read-Model Performance
Area: Planner Intelligence
Status: Open
Priority: P2

## Requirements

Track observed index selectivity and cost so the planner can choose among competing indexes more accurately.
Feedback is advisory planner input; it must never become a correctness dependency for index use.

## Dependencies

- Depends on phase 03 issue 02 for the cost-informed planning hook and cost diagnostic surface.
- Uses existing metrics feedback and index metadata persistence patterns.

## Handoff

- Provides observed selectivity and cost records consumed by phase 03 issue 02 cost-informed planning, phase 03 issue 10 advanced cardinality estimation, and future index-maintenance diagnostics.

## Functional Scope

- Record per-index feedback for predicate shape fingerprint, estimated rows, scanned index entries, fetched row blobs, returned rows, elapsed time, exact recheck count, and fallback reason.
- Partition feedback by database, collection, index id/version, schema epoch, cost-model version, predicate shape, and relevant planner feature flags.
- Use feedback in cost-informed planning to prefer indexes that have lower observed cost for matching shapes.
- Invalidate or ignore feedback when index definition, schema epoch, cost-model version, collection, or database changes.
- Bound stored feedback volume with deterministic replacement/aggregation so repeated ad hoc predicates do not create unbounded storage or metric labels.
- Expose per-index feedback through metrics and EXPLAIN diagnostics without leaking bind values or raw predicate literals.

## Non-Goals

- Do not change row blob truth or index correctness semantics.
- Do not implement automatic index creation/drop decisions here.
- Do not feed back data from failed or cancelled queries as successful selectivity evidence.

## Acceptance Criteria

- Index feedback is captured after indexed query execution and ignored for full-scan fallback.
- Competing index selection can change when feedback consistently favors one index.
- Stale or missing feedback falls back to static estimates.
- Metrics show feedback reads, writes, invalidations, and selected index influence.
- Feedback records are privacy-preserving and bounded in label/cardinality growth.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback capture, competing index choice, stale invalidation, missing fallback, failed/cancelled query exclusion, bounded aggregation, privacy/no bind values, and EXPLAIN diagnostics.
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
