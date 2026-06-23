# Phase 09 Issue 08: Byte-Accurate Capacity Diagnostics

Milestone: Production Depth And Operational Orchestration
Area: Capacity Management
Status: Open
Priority: P2

## Goal

Add byte-accurate capacity diagnostics for the storage families and read-model sidecars that currently require host-level measurement.

## Dependencies

- Phase 08 capacity-management documentation baseline is complete.
- Existing runtime metrics expose operation counts, cache occupancy, fallback counters, projection work, and column-batch byte totals.

## Requirements

- Report data bytes by relevant Midge family where available.
- Report row blob, scalar index, full-text, vector sidecar, column-batch, projection metadata, and temporary/rebuild target bytes where feasible.
- Keep byte accounting advisory and local to a single Cassie data directory.
- Avoid introducing a second storage abstraction above Midge.
- Expose diagnostics through an admin/runtime surface with deterministic tests.

## Acceptance Criteria

- Operators can inspect byte usage by major Cassie storage category.
- Capacity docs no longer require host-level measurement for categories that Cassie can report directly.
- Missing or unsupported byte categories are explicit rather than silently omitted.
- Restart/hydration behavior is covered if diagnostics persist any metadata.

## Implementation Plan

1. Audit Midge adapter APIs and existing column-batch byte counters.
2. Write failing tests for the selected capacity report surface.
3. Implement the smallest local byte accounting surface available without changing storage ownership.
4. Update `/metrics`, catalog, or admin docs depending on the chosen surface.
5. Update capacity-management and production-readiness docs.

## Required Tests

- Focused capacity diagnostics tests.
- REST or catalog tests if exposed through those surfaces.
- `cntryl-tools validate-tests -f <touched test file>`.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm diagnostics are advisory and local.
- Confirm docs do not imply automatic capacity movement or admission control.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
