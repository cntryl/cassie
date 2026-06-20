# Issue 058: PostgreSQL Wire Protocol

Milestone: V1 - Projection Query Engine
Area: Protocols
Status: Open
Priority: P0

## Concept

`PostgreSQL wire protocol` from `docs/milestones.md`.

## Goal

Deliver complete Cassie support for PostgreSQL wire protocol within the V1 - Projection Query Engine scope.

## TDD Plan

- Add the smallest failing test that proves the concept is missing or incomplete.
- Implement only enough behavior to make that test pass.
- Add focused edge-case tests after the happy path is green.
- Refactor without broadening behavior.

## Implementation Notes

Keep PostgreSQL wire protocol primary and HTTP secondary/admin.

## Acceptance Criteria

- The concept has parser, binder, planner, executor, catalog, protocol, or storage support where applicable.
- Happy path and edge cases are covered by focused tests.
- Existing related behavior does not regress.
- Touched test files pass `cntryl-tools validate-tests`.

## Validation

- `cargo test --test pgwire_simple_query --quiet`
- `cargo test --test rest --quiet`
- `cntryl-tools validate-tests -f tests/pgwire_simple_query.rs`
- `cntryl-tools validate-tests -f tests/rest.rs`
