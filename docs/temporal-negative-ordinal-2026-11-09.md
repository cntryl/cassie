# Negative interval ordinal suffixes

PostgreSQL 18.6 chooses a `to_char` ordinal suffix from the magnitude of the
rendered field. For example, interval hours -1, -2 and -3 use `st`, `nd` and
`rd`, while -11, -12 and -13 use `th`; -21, -22 and -23 repeat the first three
suffixes. Both `th` and `TH` template variants and `FM` were checked against
the PostgreSQL oracle. Cassie's signed remainder previously selected `th` for
all negative fields. The shared suffix helper now uses the unsigned magnitude,
including for `i64::MIN`, without changing the field value or spelling.

This change does not address the separate negative interval day-field padding
difference (PostgreSQL renders -1 day as `-1` for `DD`). The SQL regression is
authored but unexecuted; the consolidated build and test gates remain pending.
