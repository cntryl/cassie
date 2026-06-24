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
Phase 04 is closed and archived in `docs/performance-contracts.md` and `issues/phase-04/README.md`.
Phase 05 write optimization is closed and archived in `docs/performance-contracts.md` and `issues/phase-05/README.md`.
Phase 06 read optimization is closed and archived in `issues/phase-06/README.md`.
Phase 07 advanced query and distributed backlog work is closed and archived in `issues/phase-07/README.md`.
Phase 08 README-goal closure is closed and archived in `issues/phase-08/README.md`.
Phase 09 production depth and operational orchestration is the active execution gate in `issues/phase-09/README.md`.

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

## Priority Policy

| Priority | Meaning |
| --- | --- |
| P0 | Required to make Cassie a credible event-sourced read-model database. Work these first. |
| P1 | Required for production trust, observability, or a core differentiator after P0. |
| P2 | Important performance, analytics, or compatibility follow-up after lifecycle safety exists. |
| P3 | Parked advanced breadth; do not start until P0/P1/P2 dependencies are resolved. |

## Coverage

### Active Execution Gate

Phases 04 through 08 are closed for the current archived scope.
Phase 09 tracks planned or planned-by-depth production depth work identified in `docs/product-roadmap.md`, `docs/read-model-gap-analysis.md`, and `docs/production-readiness.md`.
Phase 08 issue 01 is closed for the operational assignment metadata baseline; see `docs/operational-scale.md`.
Phase 08 issue 02 is closed for the local snapshot/restore baseline; see `docs/snapshot-restore.md`.
Phase 08 issue 03 is closed for the 10k/100k manual benchmark feedback baseline; see `docs/performance-contracts.md`.
Phase 08 issue 04 is closed for the projection repair workflow baseline; see `docs/feature-support.md`.
Phase 08 issue 05 is closed for the read optimization MVP baseline; see `docs/performance-contracts.md`.
Phase 08 issue 06 is closed for the time-series MVP baseline; see `docs/performance-contracts.md`.
Phase 08 issue 07 is closed for the client compatibility matrix baseline; see `docs/postgres-compatibility.md`.
Phase 08 issue 08 is closed for the procedure non-goal boundary; see `docs/feature-support.md`.
Phase 08 issue 09 is closed for production-ready classification; see `docs/production-readiness.md`.
Phase 08 issue 10 is closed for capacity management and docs reconciliation; see `docs/capacity-management.md`.
Phase 09 issue 04 is closed for narrow mixed-order and expression-index read-path depth; see `docs/performance-contracts.md`.
Phase 09 issue 05 is closed for projection handler determinism and replay contracts; see `docs/projection-replay-contracts.md`.
Phase 09 issue 06 is closed for persisted bucket-native time-series index storage; see `docs/indexes-and-constraints.md` and `docs/performance-contracts.md`.
Phase 09 issue 07 is closed for opt-in SQLAlchemy Core pgwire client probe depth; see `docs/postgres-compatibility.md`.
Phase 09 issue 08 is closed for advisory local capacity byte diagnostics; see `docs/capacity-management.md`.
Phase 09 issue 09 is closed for projection repair operator runbooks; see `docs/projection-repair-runbook.md`.
Phase 09 issue 10 is closed for adaptive planning confidence gates; see `docs/feature-support.md`.
Phase 09 issue 11 is closed for experimental surface promotion criteria; see `docs/experimental-promotion-criteria.md`.

### P1 Next

- None.

### P2 Follow-Up

- None.

### P3 Parked

- None.

## Acceptance Criteria

- Every active, uncompleted milestone bullet has exactly one linked issue.
- Completed concepts are removed from this index when their issue files are deleted.
- Issue ordering matches the current priority policy.

## Required Tests

- Run link and status checks with repository search commands rather than cargo tests.
- Confirm the index contains no references to deleted issue files and no completed implementation issues.

## Close-Out Steps

- Confirm every remaining linked issue file exists.
- Confirm no linked issue has `Status: Completed`.
- Run `rg '^Status: Completed' issues` and verify it returns no active implementation issues.
