# Issue 140: Runtime Operator Switching

Milestone: V5 - Verification & Advanced Execution
Area: Query Intelligence
Status: Open
Priority: P3

## Requirement

Switch between compatible physical operators during execution when observed work exceeds safe thresholds, without changing query semantics.

## Functional Scope

- Support switchable operator pairs only when state can be transferred or replayed safely, such as nested-loop to hash join, row scan to indexed/column path for remaining work, or scalar to batch aggregation.
- Define checkpoint and replay rules for each supported switch point.
- Respect timeout, cancellation, memory/spill budgets, and deterministic final ordering.
- Emit switch decisions, trigger reason, transferred state, and fallback through EXPLAIN ANALYZE/metrics.
- Keep a runtime control to disable operator switching for deterministic debugging.

## Non-Goals

- Do not switch to an operator that was not pre-validated for the query.
- Do not implement distributed operator migration.

## Acceptance Criteria

- Supported operator switches return identical results to no-switch execution.
- Switch thresholds trigger deterministically in tests and can be disabled.
- Partial state transfer/replay is covered for every supported switch pair.
- Errors/cancellation during switch cleanup leave no active worker state.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering each supported switch pair, disabled mode, threshold trigger, state transfer, timeout/cancellation during switch, and EXPLAIN ANALYZE diagnostics.
- Include planner, integration, and metrics tests.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document supported switch pairs and safety rules.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
