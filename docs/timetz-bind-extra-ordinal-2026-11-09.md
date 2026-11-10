# TIMETZ binary Bind extra-data diagnostic

PostgreSQL's Bind path reports unread bytes after a valid TIMETZ binary prefix
as 22P03 `incorrect binary data format in bind parameter N`, with a one-based
parameter ordinal. The TIMETZ receiver validates its time and zone before the
outer unread-byte check, so invalid prefixes preserve 22008 or 22009 instead.

Extend the existing private scalar temporal-excess classifier with the already
structured TIMETZ excess marker. Keep TIMETZ range and zone errors, array
element errors and COPY errors on their prior typed paths. First/second
parameter, NULL, prefix-priority and transaction recovery probes are authored
but unexecuted pending consolidated validation. This source-determined change
does not complete #815.
