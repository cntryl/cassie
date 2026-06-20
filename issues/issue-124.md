# Issue 124: Index Performance Feedback

Milestone: V4 - Analytical Overlay
Area: Adaptive Planning
Status: Open
Priority: P3

## Concept

`index performance feedback` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for index performance feedback within the V4 - Analytical Overlay scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Keep row blobs as truth; indexes are acceleration and must have correctness fallback.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
