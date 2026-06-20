# Issue 117: Retention Policies

Milestone: V4 - Analytical Overlay
Area: Time Series
Status: Open
Priority: P3

## Concept

`retention policies` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for retention policies within the V4 - Analytical Overlay scope.

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

- `cargo test --test scalar_functions --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/scalar_functions.rs`
