# Phase 10 Issue 02: Core Planner And Executor Hot Paths

## Status

Open.

## Goal

Optimize the highest-ranked core SQL planning and execution bottlenecks from Issue 01 without changing SQL-visible behavior.

## Dependencies

- `issues/phase-10/issue-01.md` is complete.
- `docs/performance-rebaseline-phase-10.md` identifies at least one core planner/executor bottleneck owned by this issue.

## Implementation Plan

1. Pick only the core SQL bottlenecks assigned to this issue by the Phase 10 report.
2. Write failing or regression-proving tests first when behavior, diagnostics, or metrics can regress.
3. Prefer allocation reduction, bounded row decoding, plan-cache reuse, projection pruning, access-path proof tightening, and streaming execution over new abstractions.
4. Preserve existing EXPLAIN labels and add diagnostics only when the bottleneck needs a visible proof surface.
5. Re-run the owning benchmark(s) and update the Phase 10 report with before/after evidence.

## Acceptance Criteria

- Every optimized path has before/after benchmark evidence.
- Access-path assertions remain at least as strong as before the change.
- No SQL feature scope is broadened.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
cargo bench --locked --bench <touched-benchmark> --no-run
```

Run the actual touched benchmark before close-out.
