# Issue 104: Custom Tokenizers

Milestone: V3 - Advanced Query Features
Area: Search
Status: Open
Priority: P2

## Concept

`custom tokenizers` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for custom tokenizers within the V3 - Advanced Query Features scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Preserve query correctness before adding specialized acceleration paths.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
