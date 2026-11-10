# Binary DATE and TIME receive bounds

PostgreSQL 18.6 accepts DATE infinity sentinels and finite day counts in
`[-2451545, 2145031949)`, and TIME microseconds in `[0, 86400000000]`,
including 24:00:00. Out-of-range values return 22008 with `date out of range`
or `time out of range`. A bad prefix takes priority over trailing bytes; a
valid prefix with extra bytes returns 22P03, and a short frame returns 08P01.
Pinned raw-wire probes confirmed scalar Bind and scalar/array binary COPY;
Bind reports the one-based parameter index for extra bytes, and array errors
report the one-based element index. The oracle was stopped afterward.

Jev selected shared checked DATE/TIME receive plus narrow typed propagation
(receipt `target/assembly-review/date-time-binary-receive-contract-*`). Keep
`ExtendedType::receive_binary`'s public signature and its prior short-frame
messages while rejecting invalid values. Generalize the previously reviewed
private timestamp transport to DATE/TIME, retaining exact scalar Bind ordinal,
array ordinal, COPY error and transaction recovery behavior. The prior COPY
tests that asserted generic 42601 errors are updated as behavior corrections.
TIMETZ and other codecs retain their existing paths; direct construction of a
public TypedValue is outside binary receive validation.

Authored public, Bind, array, COPY and transaction probes are unexecuted until
consolidated validation; this does not complete #815 or qualify unrelated
resource boundaries.
