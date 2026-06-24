# Phase 10 Issue 03: Write Replay Rebuild And Verification Efficiency

## Status

Open.

## Goal

Optimize write-side and projection maintenance bottlenecks ranked by Issue 01 while preserving checkpoint, freshness, verification, and repair semantics.

## Dependencies

- `issues/phase-10/issue-01.md` is complete.
- The Phase 10 report assigns write, replay, rebuild, or verification bottlenecks to this issue.

## Implementation Plan

1. Pick only bottlenecks assigned to this issue by the Phase 10 report.
2. Preserve write-path contracts in `docs/performance-contracts.md` and replay contracts in `docs/projection-replay-contracts.md`.
3. Prefer grouped writes, fewer duplicate checks, fewer unchanged index rewrites, bounded rebuild memory, and reduced verification scans where contracts allow.
4. Add or tighten write-amplification metrics only when needed to prove the optimization.
5. Update the Phase 10 report with before/after evidence.

## Acceptance Criteria

- Replay idempotency, failed-build isolation, verification correctness, and repair boundaries are unchanged.
- Every optimized path has before/after benchmark or metrics evidence.

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
