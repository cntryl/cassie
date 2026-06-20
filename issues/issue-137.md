# Issue 137: Vectorized Joins

Milestone: V5 - Verification & Advanced Execution
Area: Execution
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for vectorized joins.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
