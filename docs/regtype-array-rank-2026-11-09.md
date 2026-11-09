# Built-in object identifier array rank

PostgreSQL 18.6 resolves `int[]`, `int[][]`, and
`pg_catalog.int4[][][]` to the same array type OID 1007; `text[][]` resolves
1009. `to_regprocedure('array_fill(anyelement,integer[][])')` resolves OID
1193. Conversely, `to_regtype('_int4[]')` and an unknown base type return
NULL, and the corresponding explicit `_int4[]` cast errors with 42704.

The built-in OID resolver strips all syntactic dimension suffixes and follows
the base type's `typarray` once, for casts, nullable lookups and routine
signature arguments. The explicitly named `_int4` array has no own `typarray`,
so it remains unresolved with a suffix. Existing quoted-name, typmod and
missing-type handling remain unchanged. This uses the existing builtin `pg_type`
identity records; it neither implements user-defined type catalogs nor changes
storage or public APIs. Broader #817 and #818 work remains open.
The existing name/signature parsing and built-in lookup allocations and
interruption behavior are unchanged and remain unqualified by this correction.

PostgreSQL 18.6 was used as an external semantic oracle. Cassie regression
probes are authored but unexecuted; the consolidated ordered validation remains
pending.
