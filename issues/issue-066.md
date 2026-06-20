# Issue 066: Limit Pushdown

Milestone: V2 - Query Performance
Area: Planner
Status: Open
Priority: P1

## Concept

`limit pushdown` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for limit pushdown within the V2 - Query Performance scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Prefer measured optimization with focused benchmarks and observable plan diagnostics.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
