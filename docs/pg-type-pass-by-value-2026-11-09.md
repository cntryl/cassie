# Built-in `pg_type.typbyval`

A PostgreSQL 18.6 Homebrew oracle on a 64-bit host reports `typbyval` true for
the supported timestamp/timestamptz, float4, pg_lsn, oid and reg* scalar
identities, in addition to the previously represented boolean, integer,
float8, date and time identities. It reports false for the supported arrays,
unknown, numeric, timetz, macaddr8, text and JSON. Cassie's private vector
fallback remains false. `typbyval` itself has PostgreSQL boolean OID 16. The complete selected
built-in OID matrix is recorded in the authored catalog regression.

The catalog computes this field from the selected logical type, not length:
macaddr8 and timetz remain pass-by-reference even when other similarly sized
types are pass-by-value. Existing private vector and array behavior remains
unchanged. This reports the pinned 64-bit PostgreSQL catalog convention; it
does not claim that Cassie's internal `Value` representation has PostgreSQL's
Datum ABI, nor does it add new type identities or persist data.

The Cassie catalog and binary-wire tests are authored but unexecuted. The full
consolidated ordered validation gate and the remaining #817 work stay open.
