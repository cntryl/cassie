# Issue 119: Materialized Projections

Milestone: V4 - Analytical Overlay
Area: Materialization
Status: Open
Priority: P3

## Concept

`materialized projections` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for materialized projections within the V4 - Analytical Overlay scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Keep behavior deterministic, testable, and aligned with the milestone roadmap.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test catalog_introspection --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
