# NUMERIC round/trunc scale overload diagnostics

Status: design only. No executable validation or runtime change is included.

PostgreSQL 18.6 rejects the two-argument `round` and `trunc` calls when a
NUMERIC first argument is paired with a BIGINT scale, including typed NULLs;
it accepts SMALLINT scale by implicit widening. A NUMERIC scale is also
rejected. For example, the first case returns SQLSTATE `42883`, the full
`function round(numeric, bigint) does not exist` message and the standard
undefined-function HINT.

Cassie's checked parameter Fold already rejects a qualified BIGINT or NUMERIC
scale against the INT4 signature. Its generic semantic diagnostic currently
renders only the offending type, such as `function round(bigint) does not
exist`, and has no HINT. The binder has a separate math signature validator;
its phase, routine precedence and coverage must be established independently.
The runtime's integer conversion alone is not evidence that a particular wire
query can bypass Fold admission.

For extended Parse, a focused private diagnostic could identify bare or
`pg_catalog` `round`/`trunc` with direct NULL casts or original supplied wire
parameter OIDs establishing NUMERIC and BIGINT. It must be selected after
declared-routine handling and before generic positional coercion, with current
summaries still matching. A fixed structured diagnostic could render the full
signature and HINT through the existing admitted semantic-error frame. An
explicit conversion for this variant would have to preserve the earlier
embedded `CassieError::Execution` variant and its original message instead of
using the generic hint-bearing conversion to `CassieError::Diagnostic`.

This does not by itself cover simple Query. SELECT admission under
`app/query_parameters/admission.rs` converts the private semantic error to a
public `CassieError` before the simple wire path observes it. A source-proven
private channel is needed to retain wire-only HINT and signature information
without changing the embedded variant, misclassifying unrelated errors by
message text, losing an owner, or changing transaction recovery. Until that
channel and the binder path are established, the candidate remains unimplemented.

The Jev contract review chose private diagnostic design before implementation
(0.62 versus 0.21); its Parse/binder/ownership lead was 0.86. Evidence is in
the ignored `target/assembly-review/numeric-scale-signature-*` artifacts.
Required probes include direct typed NULLs and originally supplied parameter
OIDs, bare and qualified names, visible routine overrides, non-NULL invalid
cast precedence, accepted SMALLINT, Sync and transaction recovery, exact and
below-budget admitted HINT frames, and embedded error compatibility. Mixed and
unknown operands, other scale types, DML and full overload resolution remain
separate obligations. No claim is made that #813 is complete.
