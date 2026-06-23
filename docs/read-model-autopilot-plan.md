# Read-Model Autopilot Plan

This document is archived as the execution plan that rebaselined Cassie around the README read-model mission.
It is no longer the live source of open implementation gaps.

Current sources of truth:

- [README](../README.md): product mission and non-goals.
- [Feature Support](feature-support.md): supported and unsupported behavior.
- [Product Roadmap](product-roadmap.md): implementation status by feature area.
- [Production Readiness](production-readiness.md): readiness evidence, blockers, and promotion rules.
- [Read-Model Gap Analysis](read-model-gap-analysis.md): current delta against the README goals.
- [Capacity Management](capacity-management.md): advisory sizing signals and operator actions.
- [Phase 00 Issue Index](../issues/phase-00/issue-01.md): active issue coverage index.

## Archived Operating Rules

- Keep Cassie framed as an event-sourced read-model database.
- Treat PostgreSQL compatibility as client access and tooling support, not OLTP parity.
- Prioritize capabilities by read-model need, not by whether they resemble OLTP, OLAP, search, vector retrieval, or time-series features.
- Keep Midge as the only storage layer.
- Preserve row blobs as the correctness fallback.
- Keep source and test files under 1,000 lines; extract focused modules before adding broad behavior to near-limit files.
- Use TDD for feature work: failing `should_` test, smallest passing change, focused refactor.
- Use current-thread Tokio runtime builder tests, never `#[tokio::test]`.
- Run validation in this order: `cargo build --locked`, `cargo test --locked`, `cargo fmt --all -- --check`, then `cntryl-tools validate-tests -f <path>` for touched test files.

## Rebaseline Result

Phase 08 closed the README-goal baseline around local operational assignments, local snapshot/restore, manual benchmark feedback loops, repair planning and local repair, read optimization, time-series, client compatibility, procedure boundaries, production-readiness classification, and capacity-management documentation.

Future work should start from the active issue index, not from this archived plan.
If product docs and issue coverage disagree, update the issue index first so implementation remains mechanical.

## Stop Conditions

Pause and ask for direction when:

- A required feature decision changes persistent metadata shape in a way that is not covered by an issue.
- Existing dirty work in a touched file conflicts with the planned change.
- A validation failure appears unrelated to the slice and cannot be isolated without broad refactoring.
- A file would exceed 1,000 lines without an extraction that is outside the slice.
