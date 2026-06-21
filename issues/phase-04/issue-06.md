# Phase 04 Issue 06: Runtime Operator Switching

Milestone: Advanced Backlog
Area: Query Intelligence
Status: Open
Priority: P3

## Requirements

Switch between compatible physical operators during execution when observed work exceeds safe thresholds, without changing query semantics.
Runtime switching is the highest-risk query-intelligence feature in this backlog and must only operate on explicitly pre-validated switch pairs.

## Dependencies

- Depends on phase 04 issue 05 for adaptive plan alternatives, guard conditions, and type-checked fallback operators.
- Depends on phase 04 issue 01 for operator feedback and threshold calibration where feedback is used.
- Consumes phase 04 issue 03 merge joins, phase 04 issue 04 vectorized joins, and phase 03 issue 09 vectorized aggregation only for switch pairs that have explicit state-transfer rules.

## Handoff

- Provides a constrained runtime switching framework for future operator-specific optimization work.

## Functional Scope

- Support switchable operator pairs only when state can be transferred or replayed safely, such as nested-loop to hash join, row scan to indexed/column path for remaining work, or scalar to batch aggregation.
- Start with one explicitly named switch pair and keep every additional pair behind its own tests and eligibility guard.
- Define checkpoint and replay rules for each supported switch point.
- Respect timeout, cancellation, memory/spill budgets, and deterministic final ordering.
- Emit switch decisions, trigger reason, transferred state, and fallback through EXPLAIN ANALYZE/metrics.
- Keep a runtime control to disable operator switching for deterministic debugging.
- Guarantee that rows already emitted before a switch are not duplicated, skipped, or reordered in a SQL-visible way.

## Non-Goals

- Do not switch to an operator that was not pre-validated for the query.
- Do not implement distributed operator migration.
- Do not switch after a LIMIT/OFFSET, final sort, or side-effecting administrative operation has made replay unsafe.

## Acceptance Criteria

- Supported operator switches return identical results to no-switch execution.
- Switch thresholds trigger deterministically in tests and can be disabled.
- Partial state transfer/replay is covered for every supported switch pair.
- Errors/cancellation during switch cleanup leave no active worker state.
- Unsupported or unsafe switch opportunities continue with the original operator and report the skipped reason.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering each supported switch pair, disabled mode, threshold trigger, skipped unsafe switch, state transfer/replay, no duplicate emitted rows, timeout/cancellation during switch, and EXPLAIN ANALYZE diagnostics.
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
