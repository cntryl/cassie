# Formatted timezone fields under #815

The `TZH`, `TZM`, and `OF` formatting fields select an explicit offset east of
UTC for `to_timestamp`, and are validated for `to_date` as well. Preserve the
sign independently from the numeric hour so `-00:30` remains negative. An
explicit minute may precede its hour; repeated nonzero fields must agree.
Hours must be at most 15 and minutes below 60, with invalid displacements
returning SQLSTATE `22009`; conflicting repeated values return `22007`.

PostgreSQL 18.6 was queried in a separate Unix-socket cluster. It returned
12:30 UTC for `2000-01-01 12:00:00 -00:30`, accepted ±15:59, rejected ±16:00
and minute 60 with `22009`, and accepted `TZM TZH` ordering. It rejected
conflicting repeated hour/minute fields with `22007` and retained ordinary
trailing-input handling. Native probes are in ignored
`target/scalar-oracle/timezone-{probes,range,order}.json`.

The focused SQL and wire regressions were authored before implementation and
remain unexecuted under the deferred bundle gate; no baseline RED is claimed.
This repairs existing formatting behavior without a new public API, durable
representation, storage capability or general date-format parity claim.
