# FLOAT8 power range diagnostics

The FLOAT8 `power` function and `^` operator share a calculation kernel.
Previously its finite overflow and underflow results both reported the FLOAT8
input-range message. PostgreSQL 18.6 instead reports SQLSTATE `22003` and
`value out of range: overflow` or `value out of range: underflow`, respectively.
The kernel now distinguishes those two existing failure predicates. Its domain
checks, finite calculation, NaN and infinity handling, signed zero, argument
conversion and NULL dispatch remain unchanged. NUMERIC-to-FLOAT8 input range
errors are separate and retain their existing diagnostic.

The pinned PostgreSQL 18.6 oracle confirmed the messages for both call and
operator forms, and `NaN^0`, `1^NaN`, and negative-infinity powers. Authored
pgwire regressions cover those cases, parameter/result descriptors, Sync and
recovery; they have not been executed against Cassie. This does not qualify all
FLOAT8 edge behavior, NUMERIC power resource control, or the complete #813
acceptance criteria. Builds and tests remain deferred to the consolidated gate.
