# Issue 004: Sparse Rows

Milestone: V1 - Projection Query Engine
Area: Storage
Status: Open
Priority: P0

## Concept

`sparse rows` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for sparse rows within the V1 - Projection Query Engine scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Keep Midge as the direct storage layer and do not introduce a second storage abstraction.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f <touched-test-file>`
