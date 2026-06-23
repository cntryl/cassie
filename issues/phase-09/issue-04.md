# Phase 09 Issue 04: Read-Path Depth For Mixed Ordering And Expression Indexes

Milestone: Production Depth And Operational Orchestration
Area: Read Optimization
Status: Open
Priority: P1

## Goal

Close the next read-optimization depth gap for mixed-direction secondary ordering and richer expression-index lowering while preserving Phase 06 access-path diagnostics.

## Dependencies

- Phase 06 read optimization baseline is archived in `issues/phase-06/README.md`.
- Phase 08 read optimization MVP follow-on is complete.
- Issue 03 extraction should be completed first if touched planner/executor/test files are near the file-size limit.

## Requirements

- Add EXPLAIN-visible proof for any newly optimized mixed-direction ordering path.
- Add EXPLAIN-visible proof for any newly optimized expression-index lowering path.
- Preserve correct fallback reasons when proof is missing.
- Keep row blobs as the correctness fallback.
- Add benchmark or benchmark-owner references for the optimized query shape.
- Do not claim generic PostgreSQL planner parity.

## Acceptance Criteria

- Supported mixed-direction or expression-index query shapes select the intended access path.
- Unsupported shapes retain deterministic fallback behavior and unchanged query results.
- Metrics and EXPLAIN identify optimized versus degraded paths.
- Docs and performance contracts identify the exact supported shapes.

## Implementation Plan

1. Audit existing scalar/composite/expression index planner and executor behavior.
2. Write failing tests with `should_` names for the first supported shape and the fallback shape.
3. Implement the smallest planner/executor proof needed for that shape.
4. Add metrics or EXPLAIN fields only if existing diagnostics cannot distinguish the path.
5. Update docs and benchmark ownership references.

## Required Tests

- Focused planner tests in the relevant planner test file.
- Focused integration tests for query results, EXPLAIN, metrics, and restart metadata if index metadata changes.
- `cntryl-tools validate-tests -f <touched test file>`.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm performance docs do not imply broader ordering/expression support than implemented.
- Confirm fallback result semantics are unchanged.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
