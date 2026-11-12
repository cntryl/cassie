# Numeric scalar function contract for #813

The active finite SQL profile, current [Type Contract](type-contract.md), live
[#813](https://github.com/cntryl/cassie/issues/813), and completed exact
NUMERIC prerequisite [#812](https://github.com/cntryl/cassie/issues/812) define
this slice. This is evidence for the selected forms only; the larger
PostgreSQL math catalog is not implied. No pre-release compatibility or
migration contract applies.

## Selected behavior

| Form | Accepted inputs and result | Defined edge behavior |
|---|---|---|
| `round(numeric)` / `round(numeric, int4)` | Returns NUMERIC (OID 1700). `int2` scales widen to `int4`; negative scales are supported. Untyped string literals resolve as `int4`. | Numeric ties round away from zero. A result beyond the numeric format returns `22003`. Large scales are bounded to the NUMERIC format; PostgreSQL 18.6 returns zero for `round(5::numeric, -2147483648)`. |
| `trunc(numeric)` / `trunc(numeric, int4)` | Returns NUMERIC (OID 1700). `int2` scales widen to `int4`; negative scales are supported. Untyped string literals resolve as `int4`. | Truncates toward zero. Extreme negative scale is bounded to the NUMERIC format; `trunc(-5::numeric, -2147483648)` returns zero. |
| `round(float8)`, `trunc(float8)`, `floor(float8)`, `ceil(float8)` / `ceiling(float8)` | Returns FLOAT8 (OID 701). REAL inputs widen through the floating family. | FLOAT8 `round` follows the host's nearest-integer behavior; on the pinned PostgreSQL 18.6 host, midpoint ties are nearest-even. Round, trunc, ceil and floor preserve negative zero where their results are zero. |
| `floor(numeric)`, `ceil(numeric)` / `ceiling(numeric)` | Returns NUMERIC (OID 1700). | Uses mathematical floor/ceiling; a carry past the numeric format returns `22003`. |
| `mod(smallint, smallint)`, `mod(integer, integer)`, `mod(bigint, bigint)`, `mod(numeric, numeric)` | Returns the corresponding integral width or NUMERIC (OID 1700). Mixed integral widths resolve to the widest selected integral type. | The remainder has the dividend's sign. A zero divisor returns `22012`; `INT64_MIN % -1` returns zero. REAL/FLOAT8 overloads are undefined and return `42883`; they are rejected in binding rather than converted to exact NUMERIC by the executor. |
| `exp(numeric)`, `power(numeric, numeric)`, numeric `^` | Returns NUMERIC (OID 1700) and retains exact decimal behavior for the selected NUMERIC forms. | SQL NULL is strict. Unsupported zero/negative or negative-base fractional powers return `2201F`; overflow returns `22003`. The selected exponential argument range and workspace admission are bounded and covered below. |
| `exp(float8)`, `power(float8, float8)`, FLOAT8 `^` | Returns FLOAT8 (OID 701). | SQL NULL is strict. The pinned FLOAT8 range errors distinguish overflow and underflow with `22003`; invalid power domains return `2201F`. |

An explicit NUMERIC argument selects exact exponent behavior. FLOAT8 `power`
and `^` preserve signed zero for a negative-zero base with a positive odd
exponent, and return positive zero for a positive even exponent. Integer-only
`power`/`^` expressions follow the binder's existing PostgreSQL-style overload
resolution and are not promised to be exact NUMERIC unless a NUMERIC operand
selects that family. FLOAT8 `round(value, scale)` and `trunc(value, scale)` are
not defined; PostgreSQL exposes those scale overloads only for NUMERIC and
requires an INT4 scale (INT2 widens). Explicit INT8, NUMERIC, REAL or FLOAT8
scale arguments are rejected with `42883`.

## Source and regression evidence

- [Math signatures and result identities](../src/sql/functions/math.rs) and
  [numeric result inference](../src/sql/binder/inference_numeric.rs) select
  the finite result types and parameter identities.
- [Numeric math](../src/types/extended/numeric_math.rs) and
  [numeric power](../src/types/extended/numeric_power.rs) implement exact
  rounding, remainder, range and exponent behavior. The executor's
  [math dispatch](../src/executor/filter/math_functions.rs) retains NUMERIC
  and FLOAT8 paths separately; numeric power preflights workspace and checks
  query interruption.
- [SQL math semantics](../tests/sql_queries/functions/math/math_functions.rs)
  covers numeric half-away rounding, signed scales, float rounding, floor and
  ceiling, strict NULL propagation, integer and exact-numeric MOD, and
  numeric/FLOAT8 power.
- [Wire math descriptors and parameters](../tests/pgwire_extended/functions/math/math.rs),
  [MOD result-width metadata](../tests/pgwire_extended/functions/math/mod_result_metadata.rs),
  [FLOAT8 rounding zero signs](../tests/pgwire_extended/functions/math/rounding_zero_sign.rs),
  [typed rounding and power](../tests/pgwire_extended/functions/math/typed_math/semantics.rs),
  [result metadata](../tests/pgwire_extended/functions/math/typed_math/metadata.rs),
  [numeric domain errors](../tests/pgwire_extended/functions/math/overload_errors.rs),
  [FLOAT8 power range errors](../tests/pgwire_extended/functions/math/power_range_messages.rs),
  and [numeric power resource ownership](../src/executor/filter/math_power_resource_tests.rs)
  retain the corresponding boundary evidence.
- A local, ignored PostgreSQL 18.6 oracle receipt at
  `target/v020-evidence/813/postgresql18.6-mod-rounding-oracle.txt` records
  accepted MOD result identities and values, `42883` for REAL/FLOAT8 MOD,
  NUMERIC versus FLOAT8 tie behavior, and binary negative-zero values.

## Jev review

The semantic choice was to reject REAL/FLOAT8 MOD overloads. Jev selected this
with 0.98 probability; the official PostgreSQL 18 function signatures and the
local PostgreSQL 18.6 oracle confirmed `42883` for both forms. The binder guard
and two rejection regressions implement that choice.

The adversarial review selected FLOAT8 `power`/`^` negative-zero sign as the
next in-scope coverage gap (0.86 probability). PostgreSQL 18.6 returned the
negative-zero bit pattern for a negative-zero base raised to a positive odd
exponent and positive zero for a positive even exponent. A binary-wire
regression now covers both call forms; it remains unexecuted. Redacted Jev
requests and responses are saved under the ignored
`target/v020-evidence/813/` directory.

The current slice adds a binder rejection for built-in REAL/FLOAT8 MOD calls
and pgwire regressions for both undefined signatures plus same-session
recovery, and for the result identities across all four supported MOD widths.
It also extends the FLOAT8 signed-zero regression to include `floor(-0.0)`,
adds odd/even negative-zero tests for FLOAT8 `power` and `^`, strict NULL tests,
and pins `INT64_MIN % -1`. Numeric round/trunc tests reject explicit non-INT4
scale parameters, preserve INT2 scale parameters, and retain unknown
string-literal scale resolution. Those newly authored regressions and the
implementation change remain unexecuted under the coordinator's deferred-gate
instruction. They do not establish #813 closure; the ordered local bundle
gates and final acceptance review remain pending.
