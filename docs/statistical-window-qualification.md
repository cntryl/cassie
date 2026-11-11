# Statistical window qualification

## #806 acceptance map and partial-state boundary (2026-11-11)

The live #806 acceptance is bounded by the following existing implementation
and regression suites:

- Scalar and array `percentile_cont`/`percentile_disc`, NULL and empty input,
  interval interpolation, result identity, invalid-fraction errors, retained
  memory and cancellation are covered under
  `tests/sql_queries/aggregates/ordered_set/` and
  `tests/pgwire_extended/aggregates/ordered_set/`.
- Window `FILTER` for COUNT/SUM/BOOL_AND/BOOL_OR, including the selected
  `EXCLUDE TIES` behavior, is covered by
  `tests/sql_queries/aggregates/windows/aggregate_query/semantics.rs` and
  `tests/sql_queries/aggregates/windows/aggregate_window_oracle/semantics.rs`.
- Variance/stddev numeric exactness, NULL/sample/population behavior, resource
  admission, cancellation and partial-state merge behavior are covered by
  `tests/sql_queries/aggregates/statistical/` and
  `src/executor/execution/aggregate_exec/statistical.rs`.
- The twelve regression aggregates are implemented over selected frames and
  covered by `tests/sql_queries/aggregates/statistical/windows.rs` and
  `tests/pgwire_extended/aggregates/windows/statistical_filter.rs`, including
  pairwise NULLs, filter-before-argument evaluation, OIDs, parameters,
  `EXCLUDE TIES`, empty frames and non-finite values.

For the #806 phrase “partial-state merges,” the selected scope is statistical
variance/stddev and regression accumulator states. Ordered-set percentile calls
retain and order their input values through the serial ordered-set path; no
percentile partial-state merge is selected. The parallel aggregate executor
already falls back when merged states require row order. Jev selected this
interpretation with probability 0.72 (choice confidence 0.63); its request and
response are retained under ignored
`target/v020-evidence/806-statistical-windows/`. This is a bounded reading of
the existing acceptance, not evidence that ordered-set states are mergeable.

Current source and tests implement regression window calls, but the Regression
aggregates row in `docs/feature-support.md` still says window calls are
unsupported. That summary needs coordinator-owned reconciliation before #806
closure; this worktree does not edit the shared Feature Support matrix.

The selected #806 follow-on bundle extends the existing frame evaluator with
FILTER on `var_pop`, `var_samp`, `variance`, `stddev_pop`, `stddev_samp`, and
`stddev`, and with the twelve existing paired regression aggregates as windows:
`corr`, `covar_pop`, `covar_samp`, `regr_count`, `regr_slope`, `regr_intercept`,
`regr_r2`, `regr_sxx`, `regr_sxy`, `regr_syy`, `regr_avgx`, and `regr_avgy`.
The #804 aggregate-clause prerequisite is closed. Existing grouped aggregates,
window functions, frames and exclusions retain their existing contracts. This
finite bundle does not complete every family named by #806.

Statistical integer/NUMERIC inputs retain the existing NUMERIC result and exact
moment policy; REAL/FLOAT8 inputs return FLOAT8. Regression arguments use the
existing FLOAT8 `(Y, X)` signature, skip a pair if either input is SQL NULL, and
return FLOAT8 except `regr_count` returns BIGINT. Empty and singleton outcomes
reuse the current grouped transition/finalization rules. Input row order is
preserved. Registered `pg_catalog` names share the same identities.

FILTER is represented once as an optional expression on the existing window
call. It is BOOLEAN or SQL NULL, runs before either argument for each visited
frame input, and FALSE/NULL skips the input. The representation uses a defaulted, omitted-when-absent field. Absence
means no FILTER in current-format records; the serde default is not a cross-build
compatibility promise. Cassie is unreleased and does not support older binary
readers. All analysis, binding, parameter, dependency and resource
traversals must visit it.
Engine-dependent volatile call counts across different output frames are not
qualified by the finite value witnesses; within one visited frame input the
predicate must not be duplicated for paired arguments.

Existing partition/order, ROWS/RANGE/GROUPS, named-window and exclusion selection
is reused. DISTINCT and aggregate-local ORDER BY on window aggregates continue
to reject with 0A000, matching PostgreSQL 18.6. This bundle selects no new frame
syntax, aggregate names, persistent types or Midge capabilities.

The scalar frame fold must pre-admit argument evaluation and numeric transition
and finalization workspace, keep retained states owned, check interruption at
row boundaries, and transfer output ownership into the returned rows before
releasing temporary state. No partially completed relation is published after
cancellation or resource failure. The finite workspace and output-lifetime
witnesses are the focused probes for Jev's 0.68 ownership lead.

Baseline: frozen query `6ff89ceec1da93c85b45480c992d7f1043a4b176` plus the
ordered-read compiler correction from `ba05e9fd`. Before runtime work, the
PostgreSQL 18.6 batch recorded filtered integer/FLOAT8 frames, all twelve paired
regression outputs, pairwise NULLs, empty frames, argument short-circuiting,
qualified names, mixed numeric inputs, non-finite values, EXCLUDE TIES and exact
unsupported/type SQLSTATEs. Jev selected the explicit field (1.00) and finite
bundle (0.99). Receipts remain under ignored
`target/v020-evidence/806-statistical-windows/`.

The regressions listed above are implemented in Cassie and execute against the
current runtime. On `4216ea32`, the ordered local gates pass:
`cargo build --locked --bin cassie`, `cargo test --locked`, strict workspace
Clippy, formatting, and `cntryl-tools validate-tests` for 18 relevant test files
(81/81 compliant tests). The PostgreSQL 18.6 oracle records the selected
semantic contract; Cassie's passing tests provide the runtime, resource and
cancellation evidence.

## Durable carriers and current-format reader

Views and materialized projections store raw query SQL; functions/procedures
store raw bodies. Their metadata carriers are `catalog/programs.rs` and
`catalog/collections.rs`. Each is reparsed by the current build. Existing
COUNT/SUM/Boolean FILTER lowering for raw SQL remains supported by the selected
current contract.

Index expressions/predicates are the serialized Expr carrier. A window call can
enter an Expr only through a subquery. The SQL index binder rejects subqueries
(`schema_indexes.rs`); the low-level encoded index publication guard must enforce
the same 0A000 restriction before metadata is written and when current-format
metadata is read back. This blocks filtered and unfiltered window subqueries,
including programmatically constructed index metadata. Malformed or unsupported
records fail deterministically. No older Cassie reader is a compatibility input.

Jev selected this bounded carrier handling at 0.99 after exact source review.
The field and traversal changes are part of the current public API. Cassie is
unreleased, so source compatibility with prior development builds and a
migration path are not required. Keep the explicit `filter` field rather than
hiding the predicate in arguments or an unproved sidecar. Current-format
serialization, traversal, restart and resource tests remain required; execution
is pending.

## Implementation checkpoint status

The historical partial-checkpoint notes in this document are superseded by the
source and regression suites present at `4216ea32`; the complete ordered local
gate passes at that revision. Hosted final-head checks remain pending. The
shared `docs/feature-support.md` regression-aggregate row also still marks
window calls unsupported; the coordinator must reconcile that row before
closing #806. Local gates alone do not satisfy those remaining close-out items.

## Public Rust AST shape

`cassie::sql::ast::WindowFunctionCall` is public. Adding `filter` changes its
source shape, and callers in this repository must initialize or inspect the
field. Earlier unreleased Rust APIs are not compatibility inputs; no downstream
migration or compatibility compile witness is required. The field remains
explicit because it represents selected SQL semantics. Jev selected this
contract (0.97; API/reader/traversal lead 0.83); current-build AST round trips
and every predicate traversal still require source review and executable tests.
