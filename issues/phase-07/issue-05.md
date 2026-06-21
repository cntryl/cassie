# Phase 07 Issue 05: Adaptive Execution Plans

Milestone: Advanced Backlog
Area: Query Intelligence
Status: Open
Priority: P3

## Requirements

Plan safe adaptive alternatives that can choose among pre-validated execution paths based on early runtime observations.
Adaptive execution here means choosing among planned alternatives at explicit decision points, not generating new plans while a query is running.

## Dependencies

- Depends on phase 07 issue 01 for operator selection feedback when feedback is used as a signal.
- Depends on phase 03 issue 10 for cardinality estimates and confidence metadata.
- Depends on phase 03 issue 08 for executor coordination and cleanup behavior.
- Depends on phase 04 issue 06 for runtime-boundary regression rules.
- Depends on phase 06 issue 05 for access-path and executor diagnostics.
- Consumes phase 07 issue 03 merge joins and phase 07 issue 04 vectorized joins when those alternatives are implemented.

## Handoff

- Provides the pre-validated alternative framework required by phase 07 issue 06 runtime operator switching.

## Functional Scope

- Extend physical plans with explicit adaptive decision points, alternatives, guard conditions, and fallback operators.
- Allow adaptive choices for safe categories such as scan/index path, join strategy, candidate sizing, and row/column representation when semantics are identical.
- Base decisions on early observed cardinality, runtime feedback, memory pressure, and configured thresholds.
- Record the selected alternative in EXPLAIN ANALYZE and metrics.
- Ensure all alternatives are planned and type-checked before execution starts.
- Define which observations may be read at each decision point and ensure decisions occur before consuming data in a way that would make another alternative unsafe.
- Require every adaptive alternative to cite the phase 06 access-path or operator diagnostic it preserves.
- Provide a configuration switch to disable adaptive choices globally and per query/session where the local configuration model supports it.

## Non-Goals

- Do not generate arbitrary new plans mid-query; runtime operator switching after work has started is phase 07 issue 06.
- Do not adapt in ways that change result ordering, LIMIT/OFFSET semantics, or error behavior.
- Do not choose an alternative that has not passed normal planner eligibility checks.

## Acceptance Criteria

- Adaptive plans select different safe alternatives under controlled runtime observations while returning identical results.
- Disabled/adaptive-limit-one mode uses the deterministic base plan.
- Thresholds and selected alternatives are observable through EXPLAIN ANALYZE/metrics.
- Errors in one alternative do not leave partial worker/operator state behind.
- Plan cache keys distinguish adaptive-capable plans from fixed plans when configuration affects behavior.
- All alternatives expose the same output schema and SQL-visible semantics.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering adaptive branch selection, disabled mode, threshold boundaries, plan-cache behavior, identical results across alternatives, error cleanup, and diagnostics.
- Include planner, integration, and metrics tests.

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
- `cargo test --locked --test metrics_adaptive --test metrics_feedback --test metrics_runtime --test metrics_search`
- `cargo test --locked --test integration_sql_predicates --test integration_sql_ordering --test integration_sql_projection`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
