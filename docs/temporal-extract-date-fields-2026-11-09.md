# Unsupported DATE fields in EXTRACT

PostgreSQL 18.6 rejects hour, minute, second, millisecond, microsecond and
timezone fields in `EXTRACT` from DATE with 0A000 naming the DATE type. This
applies to finite dates, both date infinities and dates beyond the timestamp
range. A NULL date still returns NULL. `date_part` is a separate overload:
it widens DATE to timestamp and, for example, returns NULL for timezone on
an infinite date. The existing DATE field validation is now performed before
the `date_part` handoff for `EXTRACT`; it does not change `date_part`.

Unknown-unit diagnostics and full-range finite DATE extraction remain open:
PostgreSQL `EXTRACT` can handle dates beyond the timestamp range, while
Cassie's current supported-field path still widens to timestamp. The focused
SQL probes are authored but unexecuted pending consolidated validation.
