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

For temporal `to_char`, PostgreSQL 18.6 returns SQL NULL for a zero-byte
template before formatting the value. A nonempty template containing only an
empty quoted literal still produces empty text. The temporal evaluator now
preserves this distinction for timestamp, timestamptz, date, and interval;
other overloads are unchanged. The SQL regression is unexecuted.

For interval `DDD` and `WW`, PostgreSQL 18.6 computes an approximate span as
`(years * 12 + months) * 30 + days`, using 32-bit signed fields; `DDD` renders
that value and `WW` renders `(span - 1) / 7 + 1` with division toward zero.
The pinned oracle build uses `-fwrapv`. Its observed extrema confirm wrapping
after each 32-bit arithmetic operation: 2147483647 months renders `-30|-3`
and -2147483648 days renders `-2147483648|306783379`. The last week result
also witnesses wrapping of `span - 1`. Signed widths count the sign for these
fields; `FM` removes padding. The interval-only calculation uses explicit
wrapping and leaves date/timestamp calendar fields unchanged. Ordinary,
negative, mixed-sign, extrema, ordinal, `FM` and timestamp controls are
authored but unexecuted. This does not establish a general resource bound.

PostgreSQL 18.6 formats `MS`, `US` and `FF1` through `FF6` with fixed signed
minimum widths: the sign counts toward the width, and `FM` does not remove
zero padding. Fractional values truncate toward zero to the requested digits;
ordinal suffixes use the resulting magnitude. This applies to timestamps as
well as intervals. PostgreSQL oracle cases cover positive, negative and
sub-millisecond fractions, FM and suffixes. Cassie regressions are unexecuted.

For interval `HH` and `HH12`, PostgreSQL 18.6 displays a zero remainder
modulo 12 as `12`. The non-FM minimum width is selected from the original hour:
negative multiples such as -12 and -24 therefore render `012`, while `FM`
renders `12`. Nonmultiples retain their signed remainder and timestamp handling
is unchanged. The SQL regression covers negative, zero and positive hours and
is unexecuted.

PostgreSQL's interval formatter retains the 64-bit whole-hour component. The
SQL path now passes that exact component privately to the clock renderer,
while the public `Fields`, `Moment` and `to_char` signatures and their existing
adapter behavior remain unchanged. This fixes `HH`, `HH12`, `HH24`, `SSSS`
and meridiem fields above the signed 32-bit hour range without narrowing or
altering the minute, second, fractional or non-interval paths. PostgreSQL 18.6
oracle cases cover both sides of the i32 boundary, positive/negative large
hours, nearly full-range microseconds, meridiem and remainders. Jev selected
the private handoff (0.98); its sign/remainder, public adapter and overflow
leads are covered by the authored, unexecuted regressions. This does not
qualify arbitrary public `Moment` values or formatter allocation/cancellation.
