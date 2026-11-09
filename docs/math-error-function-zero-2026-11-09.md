# Error function zero and subnormal termination

The floating `erf` series used a strict `abs(step) < 1e-17 * abs(sum)`
termination condition. At either signed zero, both sides are zero, and for
sufficiently small subnormals the term and tolerance can both underflow to
zero; neither case terminates. `erfc` calls the same series near zero.

The zero input now returns immediately, preserving its sign. The series also
stops when its computed step is exactly zero, at which point the floating
addition cannot alter a nonzero sum; its existing nonzero convergence rule
remains unchanged. PostgreSQL 18.6 returned signed-zero and minimum-subnormal
`erf` bits unchanged, `erfc` of those values as one, and the expected
infinity/NaN results. Direct bit-level and SQL probes are authored but
unexecuted. This correction does not establish general cancellation or
resource bounds for the remaining transcendental functions.
