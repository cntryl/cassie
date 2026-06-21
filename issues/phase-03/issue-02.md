# Phase 03 Issue 02: Cost-Informed Planning

Milestone: Read-Model Performance
Area: Planner Intelligence
Status: Open
Priority: P2

## Requirements

Use statistics and runtime feedback to choose lower-cost physical plans while preserving deterministic fallback and query results.

## Functional Scope

- Define a cost model for scans, index scans, joins, sort/top-k, aggregates, search/vector scoring, and column-batch paths.
- Use cardinality stats, index stats, runtime feedback, and conservative defaults when statistics are missing.
- Select among semantically equivalent physical operators based on estimated cost and runtime limits.
- Keep plan selection deterministic for the same catalog/stats state.
- Explain estimated costs, chosen alternatives, and fallback/default reasons through EXPLAIN/metrics.

## Non-Goals

- Do not implement adaptive mid-query re-planning here.
- Do not choose a plan that can change SQL semantics for performance.

## Acceptance Criteria

- Planner chooses lower-cost eligible plans when statistics clearly favor them and falls back deterministically when stats are missing.
- EXPLAIN includes enough cost diagnostics to understand plan choice.
- Query results are identical to existing planning for all tested shapes.
- Statistics invalidation after DDL/write changes prevents stale unsafe choices.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering cost preference among scan/index/join/top-k alternatives, missing stats fallback, stale stats invalidation, deterministic repeated planning, and EXPLAIN cost output.
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
