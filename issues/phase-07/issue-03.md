# Phase 07 Issue 03: Merge Joins

Milestone: Advanced Backlog
Area: Execution
Status: Open
Priority: P3

## Requirements

Add merge join as a physical strategy for eligible equi-joins with sorted inputs.
This issue adds one new join strategy after the planner can reason about ordering, cost, and cardinality well enough to avoid speculative plan churn.

## Dependencies

- Depends on phase 03 issue 02 for cost-informed planning.
- Depends on phase 03 issue 08 for advanced parallel execution foundations where sorted inputs are produced concurrently.
- Depends on phase 03 issue 10 for cardinality estimates that compare merge join against hash and nested-loop joins.
- Depends on existing sort and join executor semantics.
- Depends on phase 04 issue 07 for read/access-path contracts and phase 06 issue 05 for plan/executor diagnostics.

## Handoff

- Provides a sorted-input join alternative that phase 07 issue 05 adaptive execution plans can pre-validate as one safe branch.

## Functional Scope

- Planner selects merge join for inner, left, right, full, semi, and anti equi-join shapes only when both sides can be produced in compatible sorted order or sorting is cheaper than alternatives.
- Executor merges sorted inputs with correct handling of duplicate keys, null join keys, unmatched rows, and projection aliases.
- Define compatible ordering in terms of join key expressions, collation/type comparison, null ordering, and projection aliases.
- Preserve existing join semantics, SQL-visible ordering guarantees, deterministic internal tie behavior where the plan advertises ordered output, and error behavior.
- Use phase 06 ordering-proof vocabulary before claiming merge join output can satisfy downstream ordering.
- Fall back to hash/nested-loop joins when join predicates are unsupported or sorted inputs are not beneficial.
- Report merge join selection, sort requirements, input rows, matched rows, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not support non-equi merge joins in this issue.
- Do not change SQL join syntax or binder semantics.
- Do not rely on merge join output ordering to satisfy a query `ORDER BY` unless the physical ordering proof is explicit.

## Implementation Plan

### Step 1: Define merge-join operator contract

- Add operator shape metadata for equi-join families and ordering requirements.
- Define required compatibility for inner/right/full/outer/semi/anti and unsupported predicates.
- Define deterministic null ordering and tie behavior in merge output.

### Step 2: Add planner-level eligibility

- Extend join decision rules to require explicit ordered inputs or explicit pre-merge sort cost.
- Add fallback paths for unsupported comparators, missing ordering, or non-equi predicates.
- Record join-sort proof in plan nodes for downstream diagnostics.

### Step 3: Implement executor merge primitive

- Add merge-state machine with duplicate-key coalescing and unmatched handling for each supported join type.
- Preserve projection aliasing and nullability behavior exactly as existing executor.
- Add clear overflow/short-circuit rules for stream termination.

### Step 4: Preserve ordering semantics

- Ensure ordered output claims only when ordering proof exists.
- Keep merge join output ordering claims separate from unrelated query-level `ORDER BY` requirements.
- Validate ordering proofs before claiming downstream `ORDER BY` preservation.

### Step 5: Metrics and diagnostics

- Add EXPLAIN labels for merge-join selection and required sort operations.
- Add counters for input rows, matched rows, output rows, and fallback reason.
- Add operator-specific debug/plan cache keys for stable comparison.

### Step 6: Tests and close-out

- Add fixture tests for all supported join shapes, duplicate/null keys, incompatible ordering, fallback, and explain evidence.
- Add planner/executor consistency tests for deterministic equivalence against existing join execution.
- Add regression tests to prevent non-equi merge join selection.

## Acceptance Criteria

- Merge join results match existing join execution for supported join types and duplicate/null-key cases.
- Planner chooses merge join only for safe equi-join shapes and cost conditions.
- Required sort operators are explicit in plans.
- EXPLAIN identifies merge join strategy and join keys.
- Unsupported collations, type comparisons, or non-equi predicates fall back deterministically.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering inner/outer/semi/anti joins, duplicate keys, null keys, incompatible ordering, pre-sorted input, sort-required input, ORDER BY proof behavior, fallback, and EXPLAIN.
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
