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
Phase 05 and phase 06 are the current intentional rebaseline for optimization work.

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
- Do not add new local issue files while any `phase-01` through `phase-04` issue remains open, unless the backlog is intentionally rebaselined as it is for phase 05 and phase 06 optimization work.

## Priority Policy

| Priority | Meaning |
| --- | --- |
| P0 | Required to make Cassie a credible event-sourced read-model database. Work these first. |
| P1 | Required for production trust, observability, or a core differentiator after P0. |
| P2 | Important performance, analytics, or compatibility follow-up after lifecycle safety exists. |
| P3 | Parked advanced breadth; do not start until P0/P1/P2 dependencies are resolved. |

## Coverage

### P0 Now

- No open P0 coverage items.

### P1 Next

- No open P1 coverage items.

### P2 Follow-Up

- [Phase 03 Issue 01: Time-Series Indexes](../phase-03/issue-01.md) - Read-Model Analytics / Time Series
- [Phase 03 Issue 02: Cost-Informed Planning](../phase-03/issue-02.md) - Read-Model Performance / Planner Intelligence
- [Phase 03 Issue 03: Index Performance Feedback](../phase-03/issue-03.md) - Read-Model Performance / Planner Intelligence
- [Phase 03 Issue 04: IVFFlat Indexes](../phase-03/issue-04.md) - Read-Model Retrieval / Vector
- [Phase 03 Issue 05: Projection Diffing](../phase-03/issue-05.md) - Read-Model Verification / Diffing
- [Phase 03 Issue 06: Column-Native Execution Paths](../phase-03/issue-06.md) - Read-Model Performance / Column Execution
- [Phase 03 Issue 07: Hybrid Row/Column Planning](../phase-03/issue-07.md) - Read-Model Performance / Hybrid Planning
- [Phase 03 Issue 08: Advanced Parallel Execution](../phase-03/issue-08.md) - Read-Model Performance / Execution
- [Phase 03 Issue 09: Vectorized Aggregation](../phase-03/issue-09.md) - Read-Model Performance / Execution
- [Phase 03 Issue 10: Advanced Cardinality Estimation](../phase-03/issue-10.md) - Read-Model Performance / Query Intelligence
- [Phase 03 Issue 11: Projection Comparison](../phase-03/issue-11.md) - Read-Model Verification / Distributed Read Models
- [Phase 03 Issue 12: Analytical Projections](../phase-03/issue-12.md) - Read-Model Analytics / Advanced Analytics
- [Phase 03 Issue 13: Large-Scale Aggregations](../phase-03/issue-13.md) - Read-Model Analytics / Advanced Analytics
- [Phase 05 Issue 01: Write Performance Contracts](../phase-05/issue-01.md) - Read-Model Write Optimization / Contracts
- [Phase 05 Issue 02: Replay And Ingest Batching](../phase-05/issue-02.md) - Read-Model Write Optimization / Write Path and Duplicate Replay Skip
- [Phase 05 Issue 03: Index Maintenance Batching](../phase-05/issue-03.md) - Read-Model Write Optimization / Indexes
- [Phase 05 Issue 04: Write-Locality Key Layout](../phase-05/issue-04.md) - Read-Model Write Optimization / Storage Layout
- [Phase 05 Issue 05: Bulk Rebuild Fast Paths](../phase-05/issue-05.md) - Read-Model Write Optimization / Rebuild
- [Phase 05 Issue 06: Write Amplification Diagnostics](../phase-05/issue-06.md) - Read-Model Write Optimization / Diagnostics
- [Phase 06 Issue 01: Read Access-Path Contracts](../phase-06/issue-01.md) - Read-Model Read Optimization / Contracts
- [Phase 06 Issue 02: Predicate Order Limit Pushdown](../phase-06/issue-02.md) - Read-Model Read Optimization / Planner
- [Phase 06 Issue 03: Keyset Pagination](../phase-06/issue-03.md) - Read-Model Read Optimization / Executor
- [Phase 06 Issue 04: Top-K And Early Stop Execution](../phase-06/issue-04.md) - Read-Model Read Optimization / Executor
- [Phase 06 Issue 05: Projection-Shaped Read Layouts](../phase-06/issue-05.md) - Read-Model Read Optimization / Projections
- [Phase 06 Issue 06: Access-Path Assertions And Diagnostics](../phase-06/issue-06.md) - Read-Model Read Optimization / Observability

### P3 Parked

- [Phase 04 Issue 01: Operator Selection Feedback](../phase-04/issue-01.md) - Advanced Backlog / Planner Intelligence
- [Phase 04 Issue 02: Full Column-Store Tables](../phase-04/issue-02.md) - Advanced Backlog / Column Tables
- [Phase 04 Issue 03: Merge Joins](../phase-04/issue-03.md) - Advanced Backlog / Execution
- [Phase 04 Issue 04: Vectorized Joins](../phase-04/issue-04.md) - Advanced Backlog / Execution
- [Phase 04 Issue 05: Adaptive Execution Plans](../phase-04/issue-05.md) - Advanced Backlog / Query Intelligence
- [Phase 04 Issue 06: Runtime Operator Switching](../phase-04/issue-06.md) - Advanced Backlog / Query Intelligence
- [Phase 04 Issue 07: Multi-Instance Consistency Checks](../phase-04/issue-07.md) - Advanced Backlog / Distributed Read Models

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
