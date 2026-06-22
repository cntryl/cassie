# Phase 00 Issue 01: Prioritized Issue Coverage Index

Milestone: Read-Model Backlog
Area: Backlog Management
Status: Open
Priority: P0

## Goal

Track one open issue for every active, uncompleted concept in `docs/product-roadmap.md`.

## Requirements

Maintain a live coverage index that links every active roadmap concept to its implementation issue and removes concepts once their issue files are completed and deleted.
Keep the coverage index ordered by execution priority so autopilot can work from top to bottom without re-triage.
Do not create additional local issue files until the current active set is completed, deleted, or intentionally rebaselined.
Phase 04 is closed and archived in `docs/performance-contracts.md` and `issues/phase-04/README.md`. Phase 06 and phase 07 are the current intentional rebaseline for read optimization and advanced backlog work. Phase 05 write optimization is closed and archived in `docs/performance-contracts.md` and `issues/phase-05/README.md`.

## Functional Scope

- Keep one linked issue entry for each active, uncompleted milestone concept.
- Keep completed/deleted issue files out of the coverage list.
- Use two-digit phase-local issue filenames and headings.
- Keep issues grouped under `issues/phase-0n/` directories.
- Preserve the current numbering unless the backlog is intentionally rebaselined.
- Update this index when roadmap concepts are removed, completed, or split.
- Group issues by current priority: P0 now, P1 next, P2 follow-up, P3 parked.

## Non-Goals

- Do not track detailed implementation requirements in this index; those belong in the individual issue files.
- Do not keep broken links to deleted completed issue files.
- Do not add new local issue files while any `phase-01` through `phase-03` issue remains open, unless the backlog is intentionally rebaselined as it is for phase 06 and phase 07 work.

## Priority Policy

| Priority | Meaning |
| --- | --- |
| P0 | Required to make Cassie a credible event-sourced read-model database. Work these first. |
| P1 | Required for production trust, observability, or a core differentiator after P0. |
| P2 | Important performance, analytics, or compatibility follow-up after lifecycle safety exists. |
| P3 | Parked advanced breadth; do not start until P0/P1/P2 dependencies are resolved. |

## Coverage

### Active Execution Gate

This rebaseline keeps the engine-safe implementation order mechanical:

1. `phase-06/issue-01` through `phase-06/issue-05` (read implementation gates)
2. `phase-07` issues only after the phase-06 gates are closed.

### P0 Now

- No open P0 coverage items.

### P1 Next

- No open P1 coverage items.

### P2 Follow-Up

- Phase 03 implementation is complete and closed. Issues 01–13 were completed and removed from active follow-up after validation and close-out.
- Phase 04 foundation contracts are complete and closed. The archived contract surface lives in `docs/performance-contracts.md` and `issues/phase-04/README.md`.
- Phase 05 write optimization is complete and closed. The contracts, benchmarks, and diagnostics are archived in `docs/performance-contracts.md`.
- [Phase 06 Issue 01: Predicate Order Limit Pushdown](../phase-06/issue-01.md) - Read-Model Read Optimization / Planner
- [Phase 06 Issue 02: Keyset Pagination](../phase-06/issue-02.md) - Read-Model Read Optimization / Executor
- [Phase 06 Issue 03: Top-K And Early Stop Execution](../phase-06/issue-03.md) - Read-Model Read Optimization / Executor
- [Phase 06 Issue 04: Projection-Shaped Read Layouts](../phase-06/issue-04.md) - Read-Model Read Optimization / Projections
- [Phase 06 Issue 05: Access-Path Assertions And Diagnostics](../phase-06/issue-05.md) - Read-Model Read Optimization / Observability

### P3 Parked

Phase 07 remains parked until the relevant Phase 04 foundation gate and Phase 06 read-implementation gate named by each issue are complete. Phase 05 write optimization is closed and archived.

- [Phase 07 Issue 01: Operator Selection Feedback](../phase-07/issue-01.md) - Advanced Backlog / Planner Intelligence
- [Phase 07 Issue 02: Full Column-Store Tables](../phase-07/issue-02.md) - Advanced Backlog / Column Tables
- [Phase 07 Issue 03: Merge Joins](../phase-07/issue-03.md) - Advanced Backlog / Execution
- [Phase 07 Issue 04: Vectorized Joins](../phase-07/issue-04.md) - Advanced Backlog / Execution
- [Phase 07 Issue 05: Adaptive Execution Plans](../phase-07/issue-05.md) - Advanced Backlog / Query Intelligence
- [Phase 07 Issue 06: Runtime Operator Switching](../phase-07/issue-06.md) - Advanced Backlog / Query Intelligence
- [Phase 07 Issue 07: Multi-Instance Consistency Checks](../phase-07/issue-07.md) - Advanced Backlog / Distributed Read Models

## Acceptance Criteria

- Every active, uncompleted milestone bullet has exactly one linked issue.
- New milestone bullets are deferred until the current local issue set is complete or intentionally rebaselined.
- Completed concepts are removed from this index when their issue files are deleted.
- Issue ordering matches the current priority policy.

## Required Tests

- Run link and status checks with repository search commands rather than cargo tests.
- Confirm the index contains no references to deleted issue files and no completed implementation issues.
- Confirm every linked implementation issue keeps current validation commands and close-out steps.

## Close-Out Steps

- Confirm every remaining linked issue file exists.
- Confirm no linked issue has `Status: Completed`.
- Confirm every linked implementation issue has `## Requirements`, `## Acceptance Criteria`, `## Close-Out Steps`, and `## Validation`.
- Run `rg '^Status: Completed' issues` and verify it returns no active implementation issues.
- Run `rg 'issue-[0-9]+\.md' issues/phase-00/issue-01.md` and spot-check that each link resolves.
- Run `rg 'cargo test --test (parser|planner|integration_sql|metrics|executor)\b|tests/(parser|planner|integration_sql|metrics|executor)\.rs' issues` and verify it returns no stale validation commands.
