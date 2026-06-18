# Sprint 35 - User-Defined Views and View Expansion

Previous: [Sprint 34 - REST, Operations, Packaging, and V1 Release Gate](sprint-34.md)
Next: [Sprint 36 - Stored Procedure Execution and CALL Semantics](sprint-36.md)

## Goal

Add persistent, read-only user-defined views as first-class relational objects so common SELECT shapes can be named, reused, and introspected like tables.

## Requirements

- Parse and bind `CREATE VIEW` and `DROP VIEW`.
- Persist view definitions in the catalog and hydrate them on startup.
- Expand view references through the existing SELECT planner so predicates, projections, joins, CTEs, ORDER BY, LIMIT, OFFSET, and parameters continue to work.
- Support nested views when the dependency graph is acyclic.
- Surface user-defined views through catalog introspection alongside the existing system views.
- Invalidate cached plans when a view is created, dropped, or renamed.
- Return deterministic PostgreSQL-style errors for unsupported `ALTER VIEW`, DML against views, and updatable-view semantics if they remain out of scope.

## Acceptance Criteria

- `CREATE VIEW`, `SELECT` from a view, and `DROP VIEW` work end-to-end.
- View definitions survive restart and appear in catalog metadata.
- Nested views resolve deterministically.
- Unsupported view mutations fail with stable errors instead of partial behavior.
- The sprint exits with touched-test validation, `cargo build`, and Clippy green.

## Tests

- New root tests for create/select/drop view coverage and restart hydration.
- Catalog introspection tests for view visibility.
- Binder and executor tests for nested view expansion and invalidation.
- Compatibility tests for tokio-postgres and pgwire access through a view.

## Exit Gate

This sprint is complete when the view suite is green, touched tests validate, `cargo build` passes, and Clippy is clean with warnings denied.
