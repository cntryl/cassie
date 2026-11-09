# WIDTH_BUCKET unknown operands with a known count

PostgreSQL 18.6 infers FLOAT8 for the three unknown operands in
`width_bucket($1,$2,$3,2)`; repeated operand slots keep FLOAT8. A count
expression `$1::int4` sharing the first operand causes 42P08 with INT4 versus
FLOAT8 detail. A SMALLINT count also selects FLOAT8 in PostgreSQL, but remains
outside this increment. The oracle was stopped after the probes.

Jev selected the narrow no-anchor, known-INT4 count extension (receipt
`target/assembly-review/width-bucket-unknown-operands-contract-*`). The
preceding mixed-anchor gate now selects FLOAT8 when its first three arguments
are all qualified, fresh, nonliteral direct unknown parameters and the count
is a qualified known INT4. Existing ordered occurrence coercion handles repeated
operand slots and a count expression that resolved one of those slots during
its own analysis; visible declared routines retain precedence.

SMALLINT counts, mixed or unresolved other shapes, pre-existing legacy or
contextual NULL parameter states and broader #813 remain open. The probes are
authored but unexecuted pending consolidated validation.
