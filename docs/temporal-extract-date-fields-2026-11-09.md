# Unsupported DATE fields in EXTRACT

PostgreSQL 18.6 rejects hour, minute, second, millisecond, microsecond and
timezone fields in `EXTRACT` from DATE with 0A000 naming the DATE type. This
applies to finite dates, both date infinities and dates beyond the timestamp
range. A NULL date still returns NULL. `date_part` is a separate overload:
it widens DATE to timestamp and, for example, returns NULL for timezone on
an infinite date. The existing DATE field validation is now performed before
the `date_part` handoff for `EXTRACT`; it does not change `date_part`.

For recognized supported units on valid finite dates beyond the timestamp
boundary, `EXTRACT` now uses PostgreSQL's direct Julian-day calculation and
returns an exact scale-zero NUMERIC instead of widening to timestamp. `date_part`
retains its widening and range error. At the maximum supported Julian day
2,147,483,493, the next ISO year is at most 5,874,898; calendar intermediates
fit i64 and the largest absolute Unix epoch result is below 1.86e14 seconds.
The path retains existing behavior for in-range and infinite dates, unrecognized
units and other types. Jev selected this bounded route (0.89); PostgreSQL 18.6
probes cover all 14 units across the timestamp threshold, maximum DATE and ISO
year rollover, plus in-range BC, NULL and date_part controls. They are authored
but unexecuted. Unknown-unit DATE diagnostics, identifier scanning and the
ordinary NUMERIC result allocation/ownership controls remain open.
