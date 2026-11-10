# Timestamp epoch at the i64 boundary

For #815, a valid timestamp can be close enough to `i64::MAX` that adding the
946684800-second Unix epoch displacement overflows an i64. DATE_PART currently
performs that addition before converting to float, and EXTRACT first reaches
that same path. PostgreSQL 18.6 instead uses integer subtraction below its
boundary and converts the timestamp to float before subtraction above it.

Its exact EXTRACT switches at the same boundary to numeric division and then
rounds to six places. For the entire valid overflow region, the numerator has
19 decimal digits with leading base-10000 digit 922 and the denominator is
1000000 with leading digit 100. PostgreSQL's `select_div_scale` therefore uses
four fractional places before the final six-place rounding: positive Unix
microseconds round to the nearest 100, ties upward. The threshold timestamp is
`294247-01-10 04:00:54.775807` UTC. PG18.6 returned `.775806` just below it,
`.775800` at it and `.775900` at `.775850`; at the maximum timestamp it
returned `9224318016000.000000`. DATE_PART at `.999950` returned
`9223372036855.002`, demonstrating the separate floating evaluation order.
A +05:30 timestamptz for the same instant had identical epoch results. The
oracle was stopped.

Jev selected separate checked/widened numeric and PG-ordered floating branches
(`target/assembly-review/temporal-epoch-overflow-contract-*`). Preserve the
existing NULL/infinity handling and carry the original timestamptz instant into
the numeric result. Timestamptz EPOCH uses the original instant without local conversion, so even
a public fixed offset that would saturate the local intermediate cannot turn a
finite epoch into infinity. Other temporal fields and their local conversion
remain a separate existing boundary. Authored probes
are unexecuted under the consolidated build/test deferral and do not establish
#815 completion.
