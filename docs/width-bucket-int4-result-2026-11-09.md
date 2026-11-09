# Four-argument WIDTH_BUCKET result range

PostgreSQL 18.6 declares both FLOAT8 and NUMERIC four-argument WIDTH_BUCKET
results as INT4. With count 2147483647, a point at or beyond the terminal bound
would produce 2147483648 and therefore returns 22003 `integer out of range` in
both ascending and descending ranges. An interior point whose result is
2147483647 succeeds, a point below the first bound returns zero, and NULL is
strict. Count validation still precedes NaN validation. PostgreSQL rejects a
BIGINT count overload as 42883. The oracle was stopped after these probes.

Jev selected a SQL-dispatch-only postcondition on successful four-argument
results (`target/assembly-review/width-bucket-int4-result-contract-*`). It runs
after the existing float/numeric computations and their errors, rejects a result
outside INT4 with the existing 22003 diagnostic, and preserves the public i64
numeric helper and the two-argument array route. Count coercion and acceptance of
input counts above i32::MAX remain separate open obligations; this change does
not claim complete overload qualification. SQL and wire probes are authored but
unexecuted under the consolidated gate deferral; this does not complete #813.
