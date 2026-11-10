# Numeric rounding of obvious zeroes

For #813, PostgreSQL 18.6 returned zero for `round(5::numeric, -2147483648)`
and `trunc(-5::numeric, -2147483648)`, retained half-away rounding at ±5 and
scale −1, and rendered `round(0::numeric, 1000)` with 1000 fractional digits.
The oracle was stopped after the probes.

A zero mantissa can retain the requested nonnegative display scale without
constructing a power of ten. For a nonzero mantissa with bit length `b`, if the
number of dropped decimal places is greater than `b`, then `10^drop` exceeds
twice the magnitude. Both half-away rounding and truncation therefore produce
zero, with the same display scale, without constructing that divisor. Other
paths and special values retain their behavior. This avoids a specific needless
large allocation/work path; it does not make the remaining BigInt operations
budgeted or interruptible. Unit and SQL probes are authored but unexecuted under
the consolidated validation deferral. This does not complete #813.
