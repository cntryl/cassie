# Statistical window qualification

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

Source regressions are authored first; native compilation/tests/strict Clippy
remain deferred to the next frozen integrated gate. Oracle and static source
checks do not qualify Cassie execution, resource or cancellation behavior.

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

The original checkpoint was partial and did not complete source/API review,
traversal audit, pedantic cleanup, native gates or inventory. Its changes have
since been incorporated into the combined #1184 candidate, where further
traversal omissions were found and corrected. The combined source is still being
reviewed and is not qualified by the earlier checkpoint or the separate bundle
results. Compilation and tests are deferred until all open work is consolidated;
the complete ordered gate and final-head hosted checks remain pending. Do not
describe this as a completed #806 slice until its acceptance criteria are proved.

## Public Rust AST shape

`cassie::sql::ast::WindowFunctionCall` is public. Adding `filter` changes its
source shape, and callers in this repository must initialize or inspect the
field. Earlier unreleased Rust APIs are not compatibility inputs; no downstream
migration or compatibility compile witness is required. The field remains
explicit because it represents selected SQL semantics. Jev selected this
contract (0.97; API/reader/traversal lead 0.83); current-build AST round trips
and every predicate traversal still require source review and executable tests.
