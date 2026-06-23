# Phase 09 Issue 05: Projection Handler Determinism And Replay Contracts

Milestone: Production Depth And Operational Orchestration
Area: Projection Lifecycle
Status: Open
Priority: P1

## Goal

Document and test the projection handler determinism and replay failure-mode contracts needed before projection lifecycle and replay can move toward production-ready status.

## Dependencies

- Projection lifecycle, replay metadata, duplicate skip, verification, repair, and snapshot/restore baselines are complete.
- `docs/production-readiness.md` lists handler determinism contracts and failure-mode guidance as blockers.

## Requirements

- Define deterministic projection handler expectations for event order, duplicate delivery, idempotency keys, schema versions, timestamps, generated ids, and non-deterministic functions.
- Define failure handling for out-of-order events, partial batches, replay conflicts, handler errors, and restart during replay.
- Add tests for the highest-risk contract gaps.
- Document which behavior is Cassie-owned versus application-handler-owned.
- Keep replay local and do not introduce external event-store ownership inside Cassie.

## Acceptance Criteria

- Projection handler determinism expectations are explicit and linked from production-readiness docs.
- Tests cover at least one replay conflict/failure path not already covered by existing lifecycle tests.
- Operators can understand when a projection is safe to rebuild, verify, and swap after replay failures.
- No docs imply Cassie owns the source event stream.

## Implementation Plan

1. Audit projection lifecycle tests, replay APIs, and docs.
2. Write failing tests for a missing determinism or failure-mode contract.
3. Implement deterministic error/reporting behavior if needed.
4. Update docs for handler-owned versus Cassie-owned responsibilities.
5. Update production-readiness blockers.

## Required Tests

- Focused projection lifecycle/replay tests.
- Restart/hydration tests if persisted replay metadata changes.
- `cntryl-tools validate-tests -f <touched test file>`.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm failure behavior is deterministic and observable.
- Confirm application-handler responsibilities remain outside Cassie storage semantics.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
