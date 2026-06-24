# Phase 10 Issue 06: Protocol API Startup Concurrency And Mixed Load Efficiency

## Status

Open.

## Goal

Optimize pgwire, HTTP, startup, concurrency, and mixed-load bottlenecks ranked by Issue 01 without weakening blocking-boundary or protocol behavior.

## Dependencies

- `issues/phase-10/issue-01.md` is complete.
- The Phase 10 report assigns protocol, API, startup, concurrency, or mixed-load bottlenecks to this issue.

## Implementation Plan

1. Pick only bottlenecks assigned to this issue by the Phase 10 report.
2. Preserve Phase 04 runtime-boundary contracts and `tests/transport_boundaries.rs` static checks.
3. Prefer fewer protocol allocations, response serialization improvements, blocking-boundary efficiency, startup hydration batching, cache reuse, and bounded concurrency work before changing public protocol behavior.
4. Keep pgwire and REST metrics explicit for optimized blocking paths.
5. Update the Phase 10 report with before/after evidence.

## Acceptance Criteria

- Pgwire and HTTP compatibility tests remain unchanged unless a deterministic diagnostic is added.
- Async transport tasks still do not call synchronous engine/storage work directly.
- Every optimized path has before/after benchmark evidence.

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
