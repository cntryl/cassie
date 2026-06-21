# Phase 06 Issue 04: Top-K And Early Stop Execution

Milestone: Read-Model Read Optimization
Area: Executor
Status: Open
Priority: P2

## Requirements

Stop read-side work as soon as the contract allows, especially for top-K pages, existence checks, and bounded candidate retrieval.

## Dependencies

- Depends on phase 06 issue 01 for access-path contracts.
- Depends on phase 06 issue 02 for bounded scan planning.

## Handoff

- Provides executor-side bounded work patterns for read contracts that should not materialize unnecessary rows.

## Functional Scope

- Add explicit early-stop behavior for top-N ordered pages, `EXISTS`, limit-bounded scans, and bounded candidate retrieval where exactness permits.
- Preserve deterministic ordering and exact result semantics.
- Report early-stop decisions through EXPLAIN/metrics where practical.

## Required Access Path

- Executor has a proof that enough rows/candidates have been seen.
- Scan/scoring stops at the smallest exact bound allowed by the query.
- `EXISTS` stops at the first qualifying row.
- Top-K over ordered storage stops at K; top-K without ordered storage remains heap-bounded but does not claim scan early-stop.

## Forbidden Access Path

- Full collection materialization for `EXISTS`.
- Reading all rows for a limit-only projected scan when `scan_limit` is proven.
- Claiming top-K early-stop when the executor still scans the full corpus.
- Approximating full-text/vector/hybrid results without an explicit approximate contract.

## Implementation Plan

### Step 1: Add failing behavior tests

- Extend `tests/executor_limits.rs`, `tests/executor_query_sources.rs`, or a new focused file if needed.
- Add `should_short_circuit_exists_after_first_match` using metrics or a deterministic scan counter once diagnostics exist.
- Add `should_stop_projected_scan_at_scan_limit` for limit-only projected reads.
- Add `should_not_label_heap_top_k_as_storage_early_stop` for unordered top-K.
- Add `should_stop_ordered_storage_top_k_when_ordering_proof_exists` after issue 02 ordering proof exists.

### Step 2: Separate top-K concepts

- In `src/planner/physical.rs`, distinguish:
  - `top_k=true`: query has ORDER BY plus LIMIT.
  - `heap_top_k=true`: executor can keep a bounded heap but may still scan all candidates.
  - `storage_top_k=true`: storage ordering proof allows early scan stop.
- Keep existing `top_k` and `top_k_limit` for compatibility; add new diagnostics only when issue 06 can expose them.

### Step 3: Tighten projected scan limits

- Reuse `scan_limit` in `src/executor/execution/projected_read.rs` and `src/executor/scan.rs`.
- Confirm `Midge::scan_rows_batched_limit` and projected scan helpers stop iterating once the limit is reached.
- Add tests around offset handling: limit-only scan stops at K; offset+limit scans at offset+K unless keyset pagination applies.

### Step 4: Short-circuit EXISTS

- Inspect `resolve_exists_expr` in `src/executor/executor.rs` and source-expression handling in `src/executor/execution/source.rs`.
- Change EXISTS subquery execution to request at most one row where SQL semantics allow it.
- Ensure correlated/lateral EXISTS still sees the correct outer row and short-circuits per outer row.
- Preserve NOT EXISTS semantics by short-circuiting once any row exists.

### Step 5: Bound scored candidate work honestly

- In `src/executor/execution/scored.rs` and `src/executor/execution/scored/vector_topk.rs`, keep heap-bounded top-K behavior separate from storage early-stop.
- Only label full-text/vector/hybrid as early-stop when candidate generation itself is bounded by an index/candidate path.
- Continue exact final scoring for all candidates required by the contract.

### Step 6: Metrics and EXPLAIN

- Add runtime counters for early-stop hits, rows avoided, EXISTS short-circuit hits, and degraded top-K scans if issue 06 introduces a read diagnostics snapshot.
- Add EXPLAIN labels such as `early_stop=scan_limit`, `early_stop=exists`, `top_k_mode=heap`, `top_k_mode=storage`, and `early_stop=none`.

### Step 7: Benchmark validation

- Extend `tier2_subsystem_executor` with `exists_short_circuit`, `limit_projected_scan`, and storage-ordered top-K cases when supported.
- Keep `tier2_subsystem_search`, `tier2_subsystem_vector`, and `tier2_subsystem_hybrid` for scored top-K candidate behavior.

## Non-Goals

- Do not approximate exact SQL result sets for interactive query paths.
- Do not introduce hidden truncation.

## Acceptance Criteria

- Eligible queries stop work after enough rows/candidates have been produced.
- `EXISTS` and similar patterns short-circuit deterministically.
- Benchmarks show reduced row work or latency for eligible patterns.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering top-N early stop, `EXISTS` short-circuit, bounded candidate retrieval, fallback, and EXPLAIN/metrics output.
- Include executor and integration coverage.

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
- `cargo test --locked --test executor_limits --test executor_query_sources --test executor_sort`
- `cargo test --locked --test integration_sql_ordering --test integration_sql_predicates --test integration_sql_explain`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_executor --no-run`
- `cargo bench --locked --bench tier2_subsystem_search --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
