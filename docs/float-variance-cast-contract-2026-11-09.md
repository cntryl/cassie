# Explicit FLOAT variance transition

The #806 FLOAT8/REAL variance and standard-deviation path currently uses
Welford mean/M2. PostgreSQL 18.6 uses a Youngs–Cramer sum/Sxx transition and
rejects finite overflow with SQLSTATE 22003. The pinned oracle returned overflow
for ordered FLOAT8 `[1e308, 1e308]` and `[1e200, -1e200]`, including a window;
`[Infinity, 1e308]` returned NaN. `[NaN, 1e308, 1e308]` returned NaN, while
the reverse order raised overflow before the NaN. Singleton Infinity produced
NaN for population results and NULL for sample results. FALSE FILTER suppressed
the overflow. NUMERIC `[Infinity, 1]` returned NaN.

This also changes ordinary last bits: ordered FLOAT8 `[1, 2, 4]` gives population
variance `1.5555555555555556` in PostgreSQL and `1.5555555555555554` with the
existing transition. Ordered `[1e16, 10000000000000002, 10000000000000004]`
gives PostgreSQL `6.222222222222221`, versus `4` under the current transition.

Jev selected a syntactic direct FLOAT8/REAL-cast-only increment (0.80; gap 0.66),
`target/assembly-review/806-float-static-provenance-{request,response}.json`.
The mode must come from the original single argument, including grouped,
modifier and window paths, never from a first-row carrier. The cast evaluator
must return only the selected FLOAT8/REAL carrier or SQL NULL; unexpected values
fail closed. Preserve exact NUMERIC, numeric-special and mixed legacy paths,
including errors and output. Untyped or typed columns without a retained cast,
folded-away casts and manually constructed or heterogeneous expressions remain
unqualified. New finite-overflow failures deliberately return a public
`CassieError::TypeInput` with PostgreSQL 22003 and its overflow message; earlier
these cases could produce a value. No other error carrier changes are selected.

Source evidence: `cast_expr::eval_cast_expr`'s JSON, collection, percentile,
character and object-ID special cases are gated by different target types.
FLOAT8 scalar/numeric-literal paths produce FLOAT8 or NULL; REAL uses
`ExtendedType::Real::cast_from`, which produces REAL or errors, with NULL
handled before casting. Constant cast evaluation uses the same cast helpers.
Grouped and modifier paths evaluate the original argument. A window's synthetic
FunctionCall currently drops its arguments, so it must explicitly preserve the
actual window argument's mode without cloning that expression. This evidence
does not yet prove all optimizer rewrite, phase callback or ownership paths.

Selected casts must also be excluded from parallel aggregation before workers
start. The current code computes partition results before its row-order
fallback; a later worker could otherwise fail on two finite large values before
an earlier NaN is seen. The selected sequential path intentionally changes
parallel eligibility, while unselected statistical calls keep the existing
eligibility behavior. The regression exercises both input orders across the
actual partition threshold without using timing as synchronization.

The selected transition is an additional variant of the existing moments enum,
not a separately allocated owner. Rust does not promise enum layout stability;
its addition may change the inline accumulator size and therefore admitted bytes
for unselected aggregates. Unchanged budget thresholds are not claimed. The
transition uses PostgreSQL's FLOAT8 counter and operation order, including very
large counts, instead of converting an integer count on each input.

Before activation, author and source-review ordinary rounding, both overflow
shapes, NaN/Infinity ordering, NULL/empty/singleton, grouped, window, FILTER,
modifier, marker survival and partial-merge/error-state probes. Existing input,
transition and result owners, interruption and resource behavior must remain
intact. BigInt arithmetic, allocator transients and other #806 obligations stay
open. Cassie tests are unexecuted until the consolidated gate.
