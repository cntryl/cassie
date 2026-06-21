# Issue 001: Prioritized Issue Coverage Index

Milestone: Read-Model Backlog
Area: Backlog Management
Status: Open
Priority: P0

## Goal

Track one open issue for every active, uncompleted concept in `docs/product-roadmap.md`.

## Requirements

Maintain a live coverage index that links every active roadmap concept to its implementation issue and removes concepts once their issue files are completed and deleted.
Keep the coverage index ordered by execution priority so autopilot can work from top to bottom without re-triage.

## Functional Scope

- Keep one linked issue entry for each active, uncompleted milestone concept.
- Keep completed/deleted issue files out of the coverage list.
- Use zero-padded sequential issue filenames and headings.
- Preserve the current numbering unless the backlog is intentionally rebaselined.
- Update this index when roadmap concepts are added, removed, completed, or split.
- Group issues by current priority: P0 now, P1 next, P2 follow-up, P3 parked.

## Non-Goals

- Do not track detailed implementation requirements in this index; those belong in the individual issue files.
- Do not keep broken links to deleted completed issue files.

## Priority Policy

| Priority | Meaning |
| --- | --- |
| P0 | Required to make Cassie a credible event-sourced read-model database. Work these first. |
| P1 | Required for production trust, observability, or a core differentiator after P0. |
| P2 | Important performance, analytics, or compatibility follow-up after lifecycle safety exists. |
| P3 | Parked advanced breadth; do not start until P0/P1/P2 dependencies are resolved. |

## Coverage

### P0 Now

- [Issue 002: Projection Source Checkpoints](issue-002.md) - Read-Model Core / Projection Lifecycle
- [Issue 003: Idempotent Replay Ingestion](issue-003.md) - Read-Model Core / Projection Lifecycle
- [Issue 004: Materialized Projections](issue-004.md) - Read-Model Core / Projection Lifecycle
- [Issue 005: Projection Versioning](issue-005.md) - Read-Model Core / Projection Lifecycle
- [Issue 006: Projection Swaps](issue-006.md) - Read-Model Core / Projection Lifecycle

### P1 Next

- [Issue 007: Row Hashes](issue-007.md) - Read-Model Core / Verification
- [Issue 008: Range Hashes](issue-008.md) - Read-Model Core / Verification
- [Issue 009: Projection Merkle Roots](issue-009.md) - Read-Model Core / Verification
- [Issue 010: Rebuild Verification](issue-010.md) - Read-Model Core / Verification
- [Issue 011: Projection Operations Views](issue-011.md) - Read-Model Core / Operations
- [Issue 012: Projection Integrity Verification](issue-012.md) - Read-Model Core / Verification
- [Issue 013: Projection Rebuild Performance Targets](issue-013.md) - Read-Model Core / Benchmarks
- [Issue 014: Mixed Search / Vector / Analytical Execution](issue-014.md) - Read-Model Retrieval / Mixed Execution

### P2 Follow-Up

- [Issue 015: Time-Series Indexes](issue-015.md) - Read-Model Analytics / Time Series
- [Issue 016: Cost-Informed Planning](issue-016.md) - Read-Model Performance / Planner Intelligence
- [Issue 017: Index Performance Feedback](issue-017.md) - Read-Model Performance / Planner Intelligence
- [Issue 018: IVFFlat Indexes](issue-018.md) - Read-Model Retrieval / Vector
- [Issue 019: Projection Diffing](issue-019.md) - Read-Model Verification / Diffing
- [Issue 020: Column-Native Execution Paths](issue-020.md) - Read-Model Performance / Column Execution
- [Issue 021: Hybrid Row/Column Planning](issue-021.md) - Read-Model Performance / Hybrid Planning
- [Issue 022: Advanced Parallel Execution](issue-022.md) - Read-Model Performance / Execution
- [Issue 023: Vectorized Aggregation](issue-023.md) - Read-Model Performance / Execution
- [Issue 024: Advanced Cardinality Estimation](issue-024.md) - Read-Model Performance / Query Intelligence
- [Issue 025: Projection Comparison](issue-025.md) - Read-Model Verification / Distributed Read Models
- [Issue 026: Analytical Projections](issue-026.md) - Read-Model Analytics / Advanced Analytics
- [Issue 027: Large-Scale Aggregations](issue-027.md) - Read-Model Analytics / Advanced Analytics

### P3 Parked

- [Issue 028: Operator Selection Feedback](issue-028.md) - Advanced Backlog / Planner Intelligence
- [Issue 029: Full Column-Store Tables](issue-029.md) - Advanced Backlog / Column Tables
- [Issue 030: Merge Joins](issue-030.md) - Advanced Backlog / Execution
- [Issue 031: Vectorized Joins](issue-031.md) - Advanced Backlog / Execution
- [Issue 032: Adaptive Execution Plans](issue-032.md) - Advanced Backlog / Query Intelligence
- [Issue 033: Runtime Operator Switching](issue-033.md) - Advanced Backlog / Query Intelligence
- [Issue 034: Multi-Instance Consistency Checks](issue-034.md) - Advanced Backlog / Distributed Read Models

## Acceptance Criteria

- Every active, uncompleted milestone bullet has exactly one linked issue.
- New milestone bullets require a new issue before implementation starts.
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
- Run `rg 'issue-[0-9]+\.md' issues/issue-001.md` and spot-check that each link resolves.
- Run `rg 'cargo test --test (parser|planner|integration_sql|metrics|executor)\b|tests/(parser|planner|integration_sql|metrics|executor)\.rs' issues` and verify it returns no stale validation commands.
