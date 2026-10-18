# Extended types

Natively represented PostgreSQL types beyond Cassie's original scalar set
(`real` and `numeric`; temporal, network, bit-string and the other families
follow) share one extension point so each family is added without touching
the engine's shared `match` statements.

## Representation

| Layer | Carrier |
| --- | --- |
| Schema | `DataType::Extended(ExtendedType)` |
| Rows inside the engine | `Value::Typed(TypedValue)` (native, e.g. `f32`) |
| Evaluator | `ScalarValue::Typed(TypedValue)` |
| Ordering, grouping, joins, index keys | `SemanticValue::Typed(TypedKey)` with PostgreSQL order |
| Storage | row-blob tag `TYPE_EXTENDED`: type OID, then the PostgreSQL binary form |
| Result rows (embedded API, pgwire text) | `Value::String` with the PostgreSQL text output |
| pgwire binary | the PostgreSQL `send`/`recv` format |

The public result boundary renders text because PostgreSQL's text protocol
does; inside the engine values stay native so comparison never re-parses text.

## Adding a family

1. Add the variant to `ExtendedType` (`src/types/extended.rs`) with its name,
   `typname`, OID, `typlen`, category and comparison family, and resolve its SQL
   spellings in `from_sql_name`.
2. Add its carrier to `TypedValue` and its key to `TypedKey`
   (`src/types/typed.rs`).
3. Implement the family module `src/types/extended/<family>.rs`: text input
   (with PostgreSQL's SQLSTATEs), canonical text output, an order key, binary
   send/receive and casts from neighbouring types.
4. Register it in `BUILTIN_TYPES` (`src/types/builtin.rs`) with `extended_type`.
5. Unit-test the laws in the family module (output of parsed input is
   canonical, order matches PostgreSQL, binary round-trips) against the
   PostgreSQL 18.6 oracle, then the SQL and wire behavior in the owning suites.

Operators, functions and aggregates over the family are added in the evaluator
and binder by the family's owning issue.

## Families with their own operators

`inet`/`cidr`, `macaddr`/`macaddr8`, `pg_lsn` and `bit`/`bit varying`
select operators by operand type. The parser keeps PostgreSQL's spellings
(`<<`, `>>`, `&`, `|`, `#`, `||`, and `BinaryOp::Typed` for `<<=`, `>>=`,
`&&`); `BinaryOp::typed_operator` names the family operator and
`types::extended::operations` evaluates it and reports its result type for
binding. Comparisons use each family's `TypedKey` order: `inet` and `cidr`
share `network_cmp`, `bit` and `bit varying` share `bit_cmp`.

`bit(n)` and `bit varying(n)` carry their length in the type
(`ExtendedType::Bit { length }`); `None` is the unconstrained type of a
`B'...'`/`X'...'` constant. Storage and parameter input apply the length as
an assignment (`22026` on mismatch); `CAST` pads or truncates.

## NUMERIC

`ExtendedType::Numeric(Option<NumericTypmod>)` carries the declared
`(precision, scale)`; values are `mantissa * 10^-scale` big integers with
PostgreSQL's display scale (`src/types/extended/numeric.rs`). Arithmetic and
result scales follow `numeric.c` (`numeric_math.rs`); result types of
operators, `sum`/`avg` and CASE/UNION branches come from `numeric_types.rs`.
Decimal literals are `numeric`; the binder folds a decimal literal compared
with a `double precision` operand to float8. Integral `numeric` values share
the integer semantic key so int/numeric joins, grouping and set operations
agree.
