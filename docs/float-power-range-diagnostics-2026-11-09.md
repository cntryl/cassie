# FLOAT8 power range diagnostics

The FLOAT8 `power` function and `^` operator share a calculation kernel.
Previously its finite overflow and underflow results both reported the FLOAT8
input-range message. PostgreSQL 18.6 instead reports SQLSTATE `22003` and
`value out of range: overflow` or `value out of range: underflow`, respectively.
The kernel now distinguishes those two existing failure predicates. Its domain
checks, finite calculation, NaN and infinity handling, signed zero, argument
conversion and NULL dispatch remain unchanged. NUMERIC-to-FLOAT8 input range
errors are separate and retain their existing diagnostic.

For these two arithmetic failures, embedded callers that inspect the public
error enum now receive `CassieError::TypeInput` with SQLSTATE `22003` rather
than `CassieError::FloatInput(FloatInputError::Range)`. This is an intentional
error-variant change to preserve PostgreSQL's distinct operation diagnostics.
Other FLOAT8 input conversion failures keep their existing variant. Platform
`pow`/`errno` behavior for subnormal results is outside this scoped correction.

The public pre-release `QueryError` enum also gains `FloatPowerOverflow` and
`FloatPowerUnderflow` variants. They preserve operation-specific provenance
through constant planning and are converted to the `TypeInput` shape above at
the embedded error boundary. Existing consumers that exhaustively match
`QueryError` must handle the new variants. Other `TypeInput` diagnostics are
not newly classified as constant-folding errors, and power range failures are
not query interruptions.

The pinned PostgreSQL 18.6 oracle confirmed the messages for both call and
operator forms, and `NaN^0`, `1^NaN`, and negative-infinity powers. Authored
pgwire regressions cover those cases, parameter/result descriptors, Sync and
recovery; they have not been executed against Cassie. This does not qualify all
FLOAT8 edge behavior, NUMERIC power resource control, or the complete #813
acceptance criteria. Builds and tests remain deferred to the consolidated gate.
