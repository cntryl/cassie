# Issue 122: Cost-Informed Planning

Milestone: V4 - Analytical Overlay
Area: Adaptive Planning
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document cost inputs and conservative defaults.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
