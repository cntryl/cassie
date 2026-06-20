# Issue 110: Runtime Plan Diagnostics

Milestone: V3 - Advanced Query Features
Area: Observability
Status: Open
Priority: P2

## Concept

`runtime plan diagnostics` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for runtime plan diagnostics within the V3 - Advanced Query Features scope.

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

- `cargo test --test metrics --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/metrics.rs`
