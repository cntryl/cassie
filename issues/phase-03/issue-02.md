# Phase 03 Issue 02: Cost-Informed Planning

Milestone: Read-Model Performance
Area: Planner Intelligence
Status: Open
Priority: P2

## Requirements

Use statistics and runtime feedback to choose lower-cost physical plans while preserving deterministic fallback and query results.
This issue establishes the planner cost contract that later phase 03 execution features plug into.

## Dependencies

- Depends on existing planner estimates, physical planning, index metadata, metrics feedback, and plan-cache invalidation behavior.
- Consumes phase 03 issue 10 advanced statistics when available, but must work with current basic statistics first.

## Handoff

- Provides the cost model and deterministic alternative-selection API used by phase 03 issue 03 index feedback, phase 03 issue 07 hybrid row/column planning, phase 03 issue 08 parallel execution, phase 03 issue 12 analytical projections, and phase 03 issue 13 large-scale aggregations.

## Functional Scope

- Define a versioned cost model for scans, index scans, joins, sort/top-k, aggregates, search/vector scoring, column-batch paths, row materialization, and derived-state reads.
- Use cardinality stats, index stats, runtime feedback, freshness/verification state, and conservative defaults when statistics are missing.
- Select among semantically equivalent physical operators based on estimated cost and runtime limits.
- Keep plan selection deterministic for the same catalog, statistics, runtime settings, and feedback snapshot.
- Invalidate or bypass cached plans when cost-relevant catalog epochs, statistics epochs, feedback versions, or projection/index freshness states change.
- Explain estimated costs, selected alternatives, rejected unsafe alternatives, and fallback/default reasons through EXPLAIN/metrics.

## Non-Goals

- Do not implement adaptive mid-query re-planning here.
- Do not choose a plan that can change SQL semantics for performance.
- Do not require advanced statistics or runtime feedback to plan correct queries.

## Acceptance Criteria

- Planner chooses lower-cost eligible plans when statistics clearly favor them and falls back deterministically when stats are missing.
- EXPLAIN includes enough cost diagnostics to understand plan choice.
- Query results are identical to existing planning for all tested shapes.
- Statistics invalidation after DDL/write changes prevents stale unsafe choices.
- Plan cache reuse respects cost-model inputs and does not reuse a plan after a relevant epoch change.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering cost preference among scan/index/join/top-k/aggregate alternatives, missing stats fallback, stale stats invalidation, cost-model versioning, plan-cache invalidation, deterministic repeated planning, rejected unsafe alternatives, and EXPLAIN cost output.
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
