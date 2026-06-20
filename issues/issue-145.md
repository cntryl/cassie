# Issue 145: Large-Scale Aggregations

Milestone: V5 - Verification & Advanced Execution
Area: Advanced Analytics
Status: Open
Priority: P3

## Requirement

Execute high-cardinality and high-volume aggregate workloads using column, vectorized, parallel, and spill-aware execution while preserving exact SQL results.

## Functional Scope

- Combine cost-informed planning, column-native scans, vectorized aggregation, parallel aggregation, and temp spill controls for large aggregate queries.
- Support grouped and ungrouped `count`, `sum`, `avg`, `min`, `max`, DISTINCT where implemented, HAVING, ORDER BY, LIMIT, and OFFSET.
- Use rollups or analytical projections only when freshness and query shape are compatible; otherwise compute from source data.
- Enforce memory, temp spill, timeout, and result limits with clear errors.
- Report selected acceleration paths, rows processed, groups created, spills, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not approximate aggregate results or use stale projections silently.
- Do not introduce a second storage engine.

## Acceptance Criteria

- Large aggregation results match exact source execution for supported query shapes.
- Memory/spill limits are respected and tested.
- Planner chooses available acceleration paths when safe and falls back when not.
- Metrics and EXPLAIN make resource use and selected paths observable.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering large grouped aggregation, high cardinality, spill behavior, timeout/limit errors, rollup/projection use, stale fallback, and deterministic output.
- Include planner, integration, and metrics tests.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for large aggregation workloads.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
