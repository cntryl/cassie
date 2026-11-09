# Negative interval ordinal suffixes

PostgreSQL 18.6 chooses a `to_char` ordinal suffix from the magnitude of the
rendered field. For example, interval hours -1, -2 and -3 use `st`, `nd` and
`rd`, while -11, -12 and -13 use `th`; -21, -22 and -23 repeat the first three
suffixes. Both `th` and `TH` template variants and `FM` were checked against
the PostgreSQL oracle. Cassie's signed remainder previously selected `th` for
all negative fields. The shared suffix helper now uses the unsigned magnitude,
including for `i64::MIN`, without changing the field value or spelling.

For interval `DD`, PostgreSQL counts the sign in the two-character minimum
width. Thus -1 through -9 render as `-1` through `-9`, not `-01` through
`-09`; `FM` continues to omit padding, and positive values retain two digits.
This field-specific correction follows PostgreSQL 18.6's `DCH_DD` formatter;
it does not change padding for clock or other template fields. The SQL
regressions are authored but unexecuted; the consolidated build and test gates
remain pending.

Interval `CC` is also distinct from a timestamp's calendar century:
PostgreSQL 18.6 truncates the interval's signed year field divided by 100
toward zero. For example, 99 and -99 years yield `00`, whereas 100 and -199
years yield `01` and `-01`. The interval-only branch preserves timestamp and BC
century handling. Its SQL regressions are likewise unexecuted.
