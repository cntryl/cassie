# Issue 031: Vectorized Joins

Milestone: Advanced Backlog
Area: Execution
Status: Open
Priority: P3

## Requirements

Execute eligible join build/probe operations in batches to reduce per-row overhead while preserving SQL join semantics.

## Functional Scope

- Add vectorized/batch kernels for equi-join key extraction, hash build/probe, match materialization, and null-key handling.
- Support inner and left joins first, with right/full/semi/anti support only when semantics are explicitly implemented and tested.
- Use batch/column inputs where available and materialize rows only for matched output or unsupported downstream operators.
- Preserve duplicate-key behavior, null semantics, projection aliases, deterministic ordering, timeout/cancellation, and memory/spill budgets.
- Report vectorized join selection, batch sizes, build/probe rows, matches, spills, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not change parser/binder join semantics.
- Do not implement non-equi vectorized joins in this issue.

## Acceptance Criteria

- Vectorized join results match scalar/hash join results for supported join types and key shapes.
- Unsupported join types or predicates fall back deterministically.
- Memory/spill limits are enforced during batch build/probe.
- Benchmarks or metrics show reduced per-row overhead for eligible joins.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering inner/left joins, duplicate keys, null keys, unmatched rows, fallback, spill/limit behavior, cancellation cleanup, and EXPLAIN diagnostics.
- Include planner and executor tests.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test planner_physical --test planner_logical --test planner_aggregates_sets`
- `cargo test --locked --test executor_parallel --test executor_query_sources --test executor_sort`
- `cargo test --locked --test integration_sql_joins --test integration_sql_join_plans --test integration_sql_aggregates`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
