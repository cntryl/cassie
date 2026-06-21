# Phase 06 Issue 01: Predicate Order Limit Pushdown

Milestone: Read-Model Read Optimization
Area: Planner
Status: Open
Priority: P2

## Requirements

Lower supported read-model predicates, ordering, and limits into bounded Midge-native scans instead of generic row scans plus late filtering/sorting.

## Dependencies

- Depends on phase 04 issue 07 for access-path contracts.
- Depends on existing planner/index metadata and physical planning surfaces.

## Handoff

- Provides the planner rules and scan shapes used by later phase 06 pagination and early-stop work.

## Functional Scope

- Recognize supported predicate/range/order/limit combinations that map to prefix or bounded range scans.
- Push down limit/top-N opportunities where ordering proof exists.
- Keep unsupported shapes on deterministic fallback plans with visible diagnostics.
- Preserve SQL semantics, ordering, and timeout behavior.

## Required Access Path

- Planner proves a supported predicate/order/limit shape.
- Physical plan records the storage access kind and any selected index or ordering proof.
- Executor passes a bounded scan request into the scan layer.
- Midge performs point, prefix, or bounded range work where the key/index layout supports it.
- EXPLAIN identifies the optimized path and the fallback reason when proof is absent.

## Forbidden Access Path

- Full collection scan followed by late filter/sort for a pattern with a proven storage path.
- Marking `predicate_pushdown=true` when filtering still happens only after row materialization.
- Applying `LIMIT` only after reading all rows when the scan could stop early.
- Hiding unsupported predicates behind optimized EXPLAIN labels.

## Implementation Plan

### Step 1: Add failing planner tests

- Extend `tests/planner_physical.rs` with:
  - `should_mark_primary_lookup_as_point_lookup`
  - `should_mark_composite_equality_as_prefix_scan`
  - `should_mark_range_filter_as_bounded_range_scan`
  - `should_mark_order_limit_as_ordered_bounded_scan_when_index_matches`
  - `should_report_fallback_when_ordering_proof_is_missing`
- Extend `tests/planner_indexes.rs` for scalar/composite index selection where index metadata is required.
- Keep tests focused on plan fields first; result-level integration comes after executor wiring.

### Step 2: Add physical access-path model

- In `src/planner/physical.rs`, add a compact enum such as `ReadAccessPath` or string-backed plan metadata if serialization compatibility is easier:
  - `CollectionScan`
  - `PointLookup`
  - `IndexSeek`
  - `PrefixScan`
  - `RangeScan`
  - `OrderedBoundedScan`
  - `ColumnBatchScan`
  - `Fallback`
- Add fields for `access_path`, `access_path_reason`, and optional `ordering_proof` only if issue 05 has not already introduced them.
- Keep existing fields (`selected_index`, `scan_limit`, `predicate_pushdown`, `top_k`) intact for compatibility.

### Step 3: Teach planner shape recognition

- Extend `selected_index` and helper logic in `src/planner/physical.rs` to distinguish:
  - all index fields constrained by equality -> seek/prefix scan
  - leading index fields constrained by equality and next field by range -> bounded range scan
  - ordering satisfied by index field order -> ordered bounded scan
- Start with simple column/literal/param predicates. Leave expression predicates and complex boolean logic as fallback unless already supported.
- Treat partial/expression indexes conservatively and require existing predicate equivalence before using them.

### Step 4: Add scan request plumbing

- Add a crate-private scan request type near `src/executor/scan.rs`, for example `ReadScanRequest`.
- Include collection, projected fields, optional filter, optional bound/range, optional limit, and expected access path.
- Update `execute_projected_filtered_read` to build a request from the physical/logical proof instead of passing loose arguments if that keeps the flow clearer.
- Preserve the current projected/column-batch scan APIs until the new request path is covered.

### Step 5: Add Midge bounded scan APIs

- Add storage helpers in `src/midge/adapter/documents.rs` only for proven shapes:
  - point row lookup by id if missing
  - prefix/range scan over an index-backed key shape where available
  - projected bounded scan with a hard limit
- If the current scalar index metadata does not have physical index entries for a planned path, stop at planner diagnostics and mark executor fallback explicit. Do not fake pushdown with a full row scan.

### Step 6: Wire EXPLAIN and diagnostics

- Extend `src/app/query.rs` EXPLAIN output with `access_path=...`, `access_path_reason=...`, `ordering_proof=...`, and `fallback=...` only when the fields exist.
- Add `tests/integration_sql_explain.rs` coverage for optimized and fallback labels.
- Ensure plan labels are absent or `fallback` when implementation cannot prove storage-native execution.

### Step 7: Benchmark and validate

- Extend `benches/tier2_subsystem_executor.rs` with one bounded prefix/range case only after executor support exists.
- Use `tier3_system_query_breakdown` to show scan/filter/sort reduction for the optimized shape.

## Non-Goals

- Do not invent pushdown for predicates Midge cannot support efficiently.
- Do not hide fallback behind misleading EXPLAIN output.

## Acceptance Criteria

- Supported query patterns select bounded scan plans instead of full scan plus late sort/filter.
- Unsupported patterns continue to return correct results with explicit fallback behavior.
- EXPLAIN/plan tests can distinguish pushdown from fallback.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering prefix/range scans, ordering proof, limit pushdown, unsupported fallback, and EXPLAIN output.
- Include planner and integration coverage.

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
- `cargo test --locked --test planner_indexes --test planner_physical --test planner_estimates`
- `cargo test --locked --test integration_sql_predicates --test integration_sql_ordering --test integration_sql_explain`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_executor --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
