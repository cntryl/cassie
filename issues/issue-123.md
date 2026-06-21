# Issue 123: Operator Selection Feedback

Milestone: V4 - Analytical Overlay
Area: Adaptive Planning
Status: Open
Priority: P3

## Requirements

Feed observed operator performance back into future operator selection without compromising deterministic planning or correctness.

## Functional Scope

- Aggregate runtime feedback by normalized operator shape, collection, schema epoch, and relevant predicate/index characteristics.
- Compare estimated versus actual rows, elapsed time, storage reads, temp writes, and memory/spill indicators.
- Adjust future cost inputs for eligible operator alternatives when feedback is fresh and statistically meaningful.
- Bound feedback influence so a single outlier cannot permanently bias planning.
- Expose feedback use, age, confidence, and ignored/outlier status through EXPLAIN/metrics.

## Non-Goals

- Do not switch operators during an already-running query; that is issue 140.
- Do not make planning depend on bind values that are not part of the normalized safe key.

## Acceptance Criteria

- Repeated workloads can influence future operator selection when feedback is consistent.
- Feedback is invalidated or ignored across schema/catalog changes and stale epochs.
- Missing, low-confidence, or outlier feedback falls back to base cost estimates.
- Query results remain identical regardless of feedback availability.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback aggregation, plan influence, stale feedback invalidation, outlier damping, missing feedback fallback, and EXPLAIN diagnostics.
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
