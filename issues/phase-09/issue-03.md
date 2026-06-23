# Phase 09 Issue 03: Large Module Extraction Gate

Milestone: Production Depth And Operational Orchestration
Area: Module Organization
Status: Open
Priority: P1

## Goal

Reduce large-file risk before Phase 09 feature depth touches planner, executor, parser, catalog, Midge, or broad integration-test surfaces.

## Dependencies

- `AGENTS.md` and `docs/module-organization.md` require source and test files to stay under 1,000 lines.
- Phase 09 issues 04 through 08 may touch modules that are near or over that limit.

## Requirements

- Run the file-size audit from `AGENTS.md`.
- Identify files over 1,000 lines and files above 900 lines that Phase 09 work is likely to touch.
- Extract focused modules only where needed for upcoming Phase 09 work.
- Avoid behavioral changes except import/module moves and tests required to prove no regression.
- Keep new modules domain-specific and aligned with existing ownership boundaries.

## Acceptance Criteria

- The audit output is recorded in the issue close-out or docs.
- No touched source or test file remains over 1,000 lines unless the issue explicitly justifies why it is not part of the upcoming Phase 09 surface.
- Existing tests pass after extraction.
- Future Phase 09 issues can add behavior without violating the file-size rule.

## Implementation Plan

1. Run `find src tests benches -type f -name '*.rs' -print0 | xargs -0 wc -l | sort -nr | head -40`.
2. Select only files likely to be touched by active Phase 09 issues.
3. Write or preserve focused regression tests before moving code when behavior is subtle.
4. Extract modules with minimal public API churn.
5. Update module docs only if ownership boundaries change.

## Required Tests

- Existing tests for extracted modules.
- New regression tests only if extraction exposes untested behavior.
- `cntryl-tools validate-tests -f <touched test file>` for touched tests.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Include the before/after file-size audit in the final close-out.
- Confirm extraction did not broaden feature scope.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
