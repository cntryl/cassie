# Interval `date_part` and `EXTRACT` units

PostgreSQL 18.6 returns the interval day field divided by seven, truncated toward
zero, for WEEK; month and time fields do not contribute. Both `date_part` and
`EXTRACT` return NULL for WEEK on infinite intervals. Timezone, day-of-week,
day-of-year, ISO weekday/year and Julian units are unsupported for intervals
with 0A000, including for both signs of infinity. PostgreSQL returns NULL for a
timestamp's timezone part on infinity; that separate behavior is unchanged.
The oracle was stopped after the probes.

The interval unit dispatch now rejects those unsupported units before its
infinity shortcut and evaluates finite WEEK from the day field. SQL probes are
authored but unexecuted under the consolidated build/test deferral. This is a
partial #815 increment and does not establish issue completion or qualification.
