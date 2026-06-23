# Phase 09 Issue 09: Repair Scope Depth And Operator Runbooks

Milestone: Production Depth And Operational Orchestration
Area: Verification And Repair
Status: Open
Priority: P2

## Goal

Mature projection repair beyond the row/range hash-rebuild baseline by adding operator runbooks and implementing the next safe repair scope only when its mutation semantics are fully specified.

## Dependencies

- Phase 08 repair workflow baseline is complete.
- `PLAN REPAIR PROJECTION` already returns deterministic plans for row, range, index, projection-version, and full-rebuild scopes.
- `REPAIR PROJECTION` currently executes only local row/range hash repair.

## Requirements

- Document operator runbooks for plan, execute, verify, audit, rollback/escalate, and unsupported scopes.
- Select at most one additional executable repair scope for this slice if safe mutation behavior is explicit.
- Keep repair admin-only, local, explicit, audited, idempotent, and post-verified.
- Keep unsupported scopes deterministic errors until safe behavior is implemented.
- Do not add automatic query-path repair or distributed repair.

## Acceptance Criteria

- Operators have runbook-quality guidance for existing and unsupported repair scopes.
- Any newly executable scope has tests for planning, execution, audit persistence, post-verification, and deterministic rejection when unsafe.
- Production-readiness blockers are updated with the remaining repair gaps.

## Implementation Plan

1. Audit repair planning, repair execution, integrity reports, repair reports, and docs.
2. Write failing tests for the runbook-covered behavior or selected new scope.
3. Implement only the selected safe local mutation path.
4. Update docs, feature support, and production-readiness blockers.
5. Preserve deterministic dry-run/error behavior for all other scopes.

## Required Tests

- Focused `tests/projection_repair.rs` coverage or a split repair test file.
- Restart/hydration tests for audit reports if new report fields are added.
- `cntryl-tools validate-tests -f <touched test file>`.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm repair is not automatic and not distributed.
- Confirm every executable repair immediately verifies and audits.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
