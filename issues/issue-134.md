# Issue 134: Merge Joins

Milestone: V5 - Verification & Advanced Execution
Area: Execution
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for sorted join workloads.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
