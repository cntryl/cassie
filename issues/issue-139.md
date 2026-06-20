# Issue 139: Adaptive Execution Plans

Milestone: V5 - Verification & Advanced Execution
Area: Query Intelligence
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document adaptive decision points and runtime controls.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
