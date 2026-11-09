# Binary timestamp receive bounds

PostgreSQL 18.6 `timestamp_recv` and `timestamptz_recv` accept the two
infinity sentinels or finite values in `[MIN_TIMESTAMP, END_TIMESTAMP)`, and
reject other finite values with 22008 `timestamp out of range`. Binary receive
validates the consumed eight-byte value before checking extra bytes: an invalid
prefix plus extra data remains 22008, while a valid prefix plus extra data is
22P03. Short frames are 08P01. Raw PG18.6 probes confirmed scalar Bind,
TIMESTAMPTZ[] element order and scalar/array binary COPY, plus valid endpoint
Bind under Asia/Kolkata, America/New_York and Pacific/Kiritimati. The oracle
was stopped afterward. PostgreSQL's timestamp2tm checks the Julian day before
zone conversion; the valid finite interval maps to its supported Julian range.

Jev selected a shared checked TIMESTAMP/TIMESTAMPTZ decoder and narrow typed
error propagation (receipt `target/assembly-review/temporal-binary-range-
contract-*`). Preserve the public `ExtendedType::receive_binary` signature and
short-frame diagnostic, while rejecting out-of-range finite values. Scalar
Bind, array elements and binary COPY preserve typed SQLSTATE and consumed-prefix
priority; array extra-data errors retain the flattened one-based element index.
COPY errors must maintain existing transaction failure, rollback and ReadyForQuery
semantics. Existing tests asserting acceptance of out-of-range TIMESTAMPTZ or
generic COPY parse errors are updated as behavior corrections. Other date/time
binary codecs and arbitrary directly constructed public TypedValue values are
outside this change.

All authored Cassie probes remain unexecuted pending consolidated gates; this
does not complete #815 or qualify unrelated decoding/resource boundaries.
