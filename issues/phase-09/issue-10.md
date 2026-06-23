# Phase 09 Issue 10: Adaptive Planning Depth And Promotion Gates

Milestone: Production Depth And Operational Orchestration
Area: Planner And Adaptive Execution
Status: Open
Priority: P2

## Goal

Define and implement the next adaptive feedback and cost-informed planning depth slice with explicit promotion gates, diagnostics, and fallback safety.

## Dependencies

- Phase 07 adaptive feedback, adaptive execution, and runtime operator switching baselines are complete.
- `docs/product-roadmap.md` marks adaptive feedback and cost-informed planning as implemented baseline with planned depth.

## Requirements

- Pick one adaptive planning depth improvement per slice.
- Define guard conditions, stale feedback handling, confidence thresholds, and fallback behavior before implementation.
- Keep adaptive behavior disabled unless the documented `CASSIE_*` controls enable it where applicable.
- Preserve SQL-visible semantics, ordering, freshness, timeout, and error behavior.
- Add EXPLAIN/metrics evidence for decisions and fallbacks.

## Acceptance Criteria

- The selected adaptive improvement has deterministic tests for selected, ignored, stale, disabled, and fallback cases.
- EXPLAIN and metrics explain why the adaptive path was or was not used.
- Production-readiness docs list the remaining promotion gates.
- No adaptive path changes query results compared with the deterministic baseline.

## Implementation Plan

1. Audit existing adaptive tests, runtime metrics, and planner diagnostics.
2. Select one depth improvement and document its guard conditions in the issue before code if needed.
3. Write failing tests for enabled and fallback behavior.
4. Implement the smallest planner/runtime change.
5. Update feature support, performance contracts, and production-readiness docs.

## Required Tests

- Focused adaptive planner/runtime tests.
- Integration tests for result equivalence where behavior changes execution strategy.
- `cntryl-tools validate-tests -f <touched test file>`.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm disabled/default behavior remains deterministic.
- Confirm fallback diagnostics are observable.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
