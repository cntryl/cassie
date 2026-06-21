# Issue 030: Merge Joins

Milestone: Advanced Backlog
Area: Execution
Status: Open
Priority: P3

## Requirements

Add merge join as a physical strategy for eligible equi-joins with sorted inputs.

## Functional Scope

- Planner selects merge join for inner, left, right, full, semi, and anti equi-join shapes only when both sides can be produced in compatible sorted order or sorting is cheaper than alternatives.
- Executor merges sorted inputs with correct handling of duplicate keys, null join keys, unmatched rows, and projection aliases.
- Preserve existing join semantics, deterministic output ordering, and error behavior.
- Fall back to hash/nested-loop joins when join predicates are unsupported or sorted inputs are not beneficial.
- Report merge join selection, sort requirements, input rows, matched rows, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not support non-equi merge joins in this issue.
- Do not change SQL join syntax or binder semantics.

## Acceptance Criteria

- Merge join results match existing join execution for supported join types and duplicate/null-key cases.
- Planner chooses merge join only for safe equi-join shapes and cost conditions.
- Required sort operators are explicit in plans.
- EXPLAIN identifies merge join strategy and join keys.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering inner/outer/semi/anti joins, duplicate keys, null keys, pre-sorted input, sort-required input, fallback, and EXPLAIN.
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
