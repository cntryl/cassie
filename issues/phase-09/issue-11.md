# Phase 09 Issue 11: Experimental Surface Promotion Criteria

Milestone: Production Depth And Operational Orchestration
Area: Production Readiness
Status: Open
Priority: P3

## Goal

Define promotion criteria for experimental catalog, limited procedure, rollup, HNSW, embedding, and related surfaces without prematurely marking them stable or production-ready.

## Dependencies

- `docs/production-readiness.md` classifies major feature families and promotion rules.
- `docs/feature-support.md` identifies experimental surfaces.

## Requirements

- Inventory experimental surfaces that are visible through SQL, pgwire, REST, catalog, EXPLAIN, or metrics.
- Define promotion criteria by surface: compatibility guarantees, tests, restart/hydration, failure behavior, benchmark evidence, and operator docs.
- Identify surfaces that should remain experimental or be narrowed.
- Do not promote any surface without linked evidence.
- Update feature support, roadmap, and production-readiness docs.

## Acceptance Criteria

- Each selected experimental surface has explicit promotion, retention, or narrowing criteria.
- Docs do not imply production readiness where blockers remain.
- Future implementation issues can promote one surface at a time using the criteria.

## Implementation Plan

1. Audit `docs/feature-support.md`, `docs/product-roadmap.md`, and `docs/production-readiness.md`.
2. Add a promotion-criteria section or companion doc.
3. Link required tests, benchmark evidence, compatibility notes, and restart coverage per surface.
4. Update roadmap status only where the evidence already supports the change.
5. Leave implementation follow-ups as future issues when evidence is missing.

## Required Tests

- Documentation link/status searches.
- Cargo tests only if this issue changes behavior or test evidence.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm no unsupported production-ready claim was added.
- Confirm every future behavior change remains in a separate issue.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
