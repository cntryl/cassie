# Issue 000: Milestone Issue Coverage Index

Milestone: V1-V5 roadmap alignment
Area: Backlog Management
Status: Open
Priority: P0

## Goal

Track one open issue for every active, uncompleted concept in `docs/milestones.md`.

## Requirements

Maintain a live coverage index that links every active roadmap concept to its implementation issue and removes concepts once their issue files are completed and deleted.

## Functional Scope

- Keep one linked issue entry for each active, uncompleted milestone concept.
- Keep completed/deleted issue files out of the coverage list.
- Preserve issue numbering; do not renumber existing open issues.
- Update this index when roadmap concepts are added, removed, completed, or split.

## Non-Goals

- Do not track detailed implementation requirements in this index; those belong in the individual issue files.
- Do not keep broken links to deleted completed issue files.

## Coverage

- [Issue 117: Retention Policies](issue-117.md) - V4 - Analytical Overlay / Time Series
- [Issue 118: Time-Series Indexes](issue-118.md) - V4 - Analytical Overlay / Time Series
- [Issue 119: Materialized Projections](issue-119.md) - V4 - Analytical Overlay / Materialization
- [Issue 120: Projection Versioning](issue-120.md) - V4 - Analytical Overlay / Materialization
- [Issue 121: Projection Swaps](issue-121.md) - V4 - Analytical Overlay / Materialization
- [Issue 122: Cost-Informed Planning](issue-122.md) - V4 - Analytical Overlay / Adaptive Planning
- [Issue 123: Operator Selection Feedback](issue-123.md) - V4 - Analytical Overlay / Adaptive Planning
- [Issue 124: Index Performance Feedback](issue-124.md) - V4 - Analytical Overlay / Adaptive Planning
- [Issue 125: IVFFlat Indexes](issue-125.md) - V4 - Analytical Overlay / Vector
- [Issue 126: Row Hashes](issue-126.md) - V5 - Verification & Advanced Execution / Merkle Overlay
- [Issue 127: Range Hashes](issue-127.md) - V5 - Verification & Advanced Execution / Merkle Overlay
- [Issue 128: Projection Merkle Roots](issue-128.md) - V5 - Verification & Advanced Execution / Merkle Overlay
- [Issue 129: Projection Diffing](issue-129.md) - V5 - Verification & Advanced Execution / Merkle Overlay
- [Issue 130: Rebuild Verification](issue-130.md) - V5 - Verification & Advanced Execution / Merkle Overlay
- [Issue 131: Full Column-Store Tables](issue-131.md) - V5 - Verification & Advanced Execution / Column Tables
- [Issue 132: Column-Native Execution Paths](issue-132.md) - V5 - Verification & Advanced Execution / Column Tables
- [Issue 133: Hybrid Row/Column Planning](issue-133.md) - V5 - Verification & Advanced Execution / Column Tables
- [Issue 134: Merge Joins](issue-134.md) - V5 - Verification & Advanced Execution / Execution
- [Issue 135: Advanced Parallel Execution](issue-135.md) - V5 - Verification & Advanced Execution / Execution
- [Issue 136: Vectorized Aggregation](issue-136.md) - V5 - Verification & Advanced Execution / Execution
- [Issue 137: Vectorized Joins](issue-137.md) - V5 - Verification & Advanced Execution / Execution
- [Issue 138: Advanced Cardinality Estimation](issue-138.md) - V5 - Verification & Advanced Execution / Query Intelligence
- [Issue 139: Adaptive Execution Plans](issue-139.md) - V5 - Verification & Advanced Execution / Query Intelligence
- [Issue 140: Runtime Operator Switching](issue-140.md) - V5 - Verification & Advanced Execution / Query Intelligence
- [Issue 141: Projection Comparison](issue-141.md) - V5 - Verification & Advanced Execution / Distributed Read Models
- [Issue 142: Projection Integrity Verification](issue-142.md) - V5 - Verification & Advanced Execution / Distributed Read Models
- [Issue 143: Multi-Instance Consistency Checks](issue-143.md) - V5 - Verification & Advanced Execution / Distributed Read Models
- [Issue 144: Analytical Projections](issue-144.md) - V5 - Verification & Advanced Execution / Advanced Analytics
- [Issue 145: Large-Scale Aggregations](issue-145.md) - V5 - Verification & Advanced Execution / Advanced Analytics
- [Issue 146: Mixed Search / Vector / Analytical Execution](issue-146.md) - V5 - Verification & Advanced Execution / Advanced Analytics

## Acceptance Criteria

- Every active, uncompleted milestone bullet has exactly one linked issue.
- New milestone bullets require a new issue before implementation starts.
- Completed concepts are removed from this index when their issue files are deleted.

## Required Tests

- Run link and status checks with repository search commands rather than cargo tests.
- Confirm the index contains no references to deleted issue files and no completed implementation issues.
- Confirm every linked implementation issue keeps current validation commands and close-out steps.

## Close-Out Steps

- Confirm every remaining linked issue file exists.
- Confirm no linked issue has `Status: Completed`.
- Confirm every linked implementation issue has `## Requirements`, `## Acceptance Criteria`, `## Close-Out Steps`, and `## Validation`.
- Run `rg '^Status: Completed' issues` and verify it returns no active implementation issues.
- Run `rg 'issue-[0-9]+\.md' issues/issue-000.md` and spot-check that each link resolves.
- Run `rg 'cargo test --test (parser|planner|integration_sql|metrics|executor)\b|tests/(parser|planner|integration_sql|metrics|executor)\.rs' issues` and verify it returns no stale validation commands.
