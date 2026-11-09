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
but unexecuted. The ordinary NUMERIC result allocation/ownership controls
remain open.

PostgreSQL 18.6 DATE EXTRACT distinguishes recognized reserved fields such as
`now`, `today` and `infinity` (0A000, not supported) from non-unit and unknown
words such as `january` and `bogus` (22023, not recognized). Both errors name
DATE, including for infinite and wide finite dates. Before classification,
PostgreSQL lowercases ASCII and clips error spelling to 63 bytes on a UTF-8
character boundary. An independently initialized UTF8 PG18.6 oracle confirmed
31 complete `é` characters survive a 40-character input, and an ASCII alias
with a recognized ten-byte prefix keeps its full clipped error spelling.

The SQL-private DATE classifier checks the unit table first, then the date
keyword table, preserving reserved-versus-unknown distinctions; supported
units continue to the existing extraction path. All keyword table spellings
are ASCII: a multibyte codepoint in the first ten bytes cannot equal an ASCII
keyword in either PG's ten-byte comparison or Cassie's ten-character key,
while ten initial ASCII bytes compare identically. Strict NULL remains first;
DATE_PART and non-DATE inputs are unchanged. Jev selected this scoped route
(0.54) conditional on the UTF8/table proof. Diagnostics have bounded spelling,
but ordinary SQL argument ownership and diagnostic allocation controls remain
open. Authored tests remain unexecuted.
