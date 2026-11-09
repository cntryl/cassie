# Built-in object identifier array rank

PostgreSQL 18.6 resolves `int[]`, `int[][]`, `integer[ ]`, `integer[2]`, and
`pg_catalog.int4[][][]` to the same array type OID 1007; `text[][]` resolves
1009. `to_regprocedure('array_fill(anyelement,integer[][])')` and
`to_regprocedure('array_fill(anyelement,integer[2])')` resolve OID 1193.
PostgreSQL REL_18_6 `regtypein` calls `parseTypeString`; `gram.y`
`opt_array_bounds` accepts repeated empty or integer-bound brackets. `scan.l`
defines `ICONST` with decimal, hexadecimal, octal and binary forms and
underscore separators. ASCII lexer whitespace and SQL comments separate the
tokens. `process_integer_literal` produces `ICONST` only when the value fits
the signed 32-bit parser representation. Conversely, `to_regtype('_int4[]')`
and an unknown base type return NULL, and the corresponding explicit `_int4[]`
cast errors with 42704.

The bounded builtin OID resolver accepts repeated empty or integer-bound
dimension suffixes and follows the base type's `typarray` once, for casts,
nullable lookups and routine signature arguments. Dimension values and rank do
not change the builtin array type OID. The explicitly named `_int4` array has no
own `typarray`, so it remains unresolved with a suffix. Existing quoted-name,
typmod and missing-type handling remain unchanged. This uses the existing
builtin `pg_type` identity records; it neither implements user-defined type
catalogs nor changes storage or public APIs. Broader #817 and #818 work remains
open.

For the first follow-up selection, Jev chose `builtin_bracket_bounds` with
probability 0.99 and judged the shared single-array identity true with
probability 0.93. A second question selected `postgres_lexical_suffix` with
probability 0.98 and judged rejection of Unicode whitespace true with
probability 0.95. The bounded behavior is to accept repeated empty or
integer-bound suffixes for built-in types, including PostgreSQL 18.6 integer
literal forms, valid separators, ASCII lexer whitespace and comments within
the suffix. Bounds must parse as a signed 32-bit integer; malformed and
overflow values and non-ASCII whitespace remain unsupported. All accepted
dimensions resolve to the existing array OID once. `_int4[]` remains
unsupported, and this does not expand to general PostgreSQL type-name grammar.

Focused Cassie probes now cover decimal, underscore, radix, comment,
non-ASCII-whitespace, malformed, overflow, `regtype`, `to_regtype` and
`regprocedure` routes. They are authored but unexecuted; the consolidated
ordered validation remains pending.
The existing name/signature parsing and built-in lookup allocations and
interruption behavior are unchanged and remain unqualified by this correction.

PostgreSQL 18.6 was used as an external semantic oracle. Cassie regression
probes are authored but unexecuted; the consolidated ordered validation remains
pending.
