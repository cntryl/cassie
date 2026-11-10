# `make_interval` seconds range

PostgreSQL 18.6 rounds seconds to microseconds using ties-to-even, then requires
the result to fit i64. It accepts the exactly representable negative i64::MIN
endpoint, but rejects positive 2^63 and values below the negative endpoint with
22008. A finite seconds value whose multiplication by one million overflows
float8 instead returns 22003; component month/week overflow is checked first.
NaN and infinities return 22008, and an interval equal to the reserved infinity
sentinel is rejected separately. Oracle probes were completed and the server
was stopped.

The constructor now uses checked conversion rather than the shared saturating
rounding helper, retaining its existing component and sentinel checks. SQL and
unit probes cover the signed boundaries, ties, overflow/error priority and
session recovery, but remain unexecuted under the consolidated gate deferral.
This is a partial #815 fix, not issue completion.
