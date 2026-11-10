# Rounded seconds in temporal constructors

PostgreSQL 18.6 validates `make_time` seconds after rounding to microseconds,
using ties-to-even. Values −0.0000001 and −0.0000005 round to zero, while
60.0000001 and 60.0000005 round to sixty seconds; the neighboring ±0.0000006
outside the allowed range are rejected with 22008. The same behavior applies
through `make_timestamp` and `make_timestamptz`, including 24:00 boundaries.
The PostgreSQL oracle was stopped after the probes.

`make_time` now checks its rounded seconds while retaining hour, minute,
nonfinite and complete-day validation. Date and timezone error ordering in its
callers is unchanged. Unit and SQL probes are authored but unexecuted under the
consolidated build/test deferral; this does not complete #815.
