# Sprint 04 - Planner, Optimizer, and Physical Plan Determinism

Previous: [Sprint 03 - SQL Parser and Binder V1](completed/sprint-03.md)  
Next: [Sprint 05 - Executor Semantics and Query Result Contract](sprint-05.md)

## Goal

Lock the query compilation pipeline so SQL always becomes a predictable logical and physical plan before execution. This sprint protects the contract that PostgreSQL wire, REST, and internal execution paths will depend on.

## Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` documents/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## Requirements

- Keep the pipeline stable: `ParsedStatement -> BoundStatement -> LogicalPlan -> PhysicalPlan`.
- Preserve collection name, projection, filter, order, limit, and offset exactly in the logical plan.
- Optimizer defaults omitted `OFFSET` to `0`.
- Physical operator order remains `Scan -> Filter -> Sort -> Project -> Offset -> Limit`.
- Do not let feature-specific operators reorder the base SQL pipeline unless tests prove equivalent semantics.
- Keep planning deterministic for equivalent input and catalog state.
- Return deterministic planner errors for unsupported plan shapes.

## Acceptance Criteria

- Planner tests prove logical shape and physical operator sequence.
- Optimizer tests prove deterministic defaults.
- Repeated planning of identical SQL produces identical plan shape.
- Collection and clause values are preserved from parse through logical planning.
- Physical plans expose enough operator information for executor tests to assert execution order.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/planner.rs`: logical plan captures collection, projection, filter, order, limit, and offset.
- `tests/planner.rs`: optimizer defaults omitted offset to `0`.
- `tests/planner.rs`: physical operator materialization follows the canonical order.
- `tests/planner.rs`: repeated planning of the same query returns the same shape.
- Keep planner tests independent from Midge data writes unless testing catalog existence.

## Exit Gate

This sprint is complete when planner, optimizer, and physical plan tests are validator-clean, `cargo test --test planner` passes, `cargo build` passes, and Clippy is clean with warnings denied.
