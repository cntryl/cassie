# Temporal constructor boundary checks

PostgreSQL 18.6 returns 22008 for an i32-minimum year in `make_date`,
`make_timestamp` and `make_timestamptz`, and for a date-valid year beyond the
timestamp range. It validates the date, then the time, before reporting a
converted timestamp-range error; an invalid time takes priority over later zone
resolution. A valid local timestamp may also cross the UTC timestamp boundary
after applying the requested timezone: PostgreSQL rejects both upper and lower
crossings with 22008 `timestamp out of range`, but accepts the same endpoints in
UTC. A valid local timestamp with an unknown explicit zone reports 22023.
PostgreSQL checks were performed and the oracle was stopped.

The constructor now uses checked year negation and date-to-microsecond
arithmetic, and revalidates a timestamptz after timezone conversion. Field and
zone ordering remain as before. The SQL regression probes, including ordinary
BCE, both UTC endpoints and session recovery, are authored but unexecuted under
the consolidated build/test deferral. This increment does not complete #815.
