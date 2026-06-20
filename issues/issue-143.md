# Issue 143: Multi-Instance Consistency Checks

Milestone: V5 - Verification & Advanced Execution
Area: Distributed Read Models
Status: Open
Priority: P3

## Concept

`multi-instance consistency checks` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for multi-instance consistency checks within the V5 - Verification & Advanced Execution scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Keep verification metadata as an overlay and avoid replacing Midge as the direct storage layer.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f <touched-test-file>`
