# Issue 139: Adaptive Execution Plans

Milestone: V5 - Verification & Advanced Execution
Area: Query Intelligence
Status: Open
Priority: P3

## Requirements

Plan safe adaptive alternatives that can choose among pre-validated execution paths based on early runtime observations.

## Functional Scope

- Extend physical plans with explicit adaptive decision points, alternatives, guard conditions, and fallback operators.
- Allow adaptive choices for safe categories such as scan/index path, join strategy, candidate sizing, and row/column representation when semantics are identical.
- Base decisions on early observed cardinality, runtime feedback, memory pressure, and configured thresholds.
- Record the selected alternative in EXPLAIN ANALYZE and metrics.
- Ensure all alternatives are planned and type-checked before execution starts.

## Non-Goals

- Do not generate arbitrary new plans mid-query; runtime operator switching is issue 140.
- Do not adapt in ways that change result ordering, LIMIT/OFFSET semantics, or error behavior.

## Acceptance Criteria

- Adaptive plans select different safe alternatives under controlled runtime observations while returning identical results.
- Disabled/adaptive-limit-one mode uses the deterministic base plan.
- Thresholds and selected alternatives are observable through EXPLAIN ANALYZE/metrics.
- Errors in one alternative do not leave partial worker/operator state behind.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering adaptive branch selection, disabled mode, threshold boundaries, identical results across alternatives, error cleanup, and diagnostics.
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
