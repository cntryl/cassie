# WIDTH_BUCKET mixed known and unknown parameters

PostgreSQL 18.6 resolves an unknown operand as FLOAT8 when the other two
operands are FLOAT8, or NUMERIC when they are NUMERIC, including different
numeric typmods. An unknown count is INT4; repeated operand parameters retain
their selected operand type. A shared operand/count parameter errors with
42P08. Mixed NUMERIC/FLOAT8 anchors select FLOAT8, and supplied SMALLINT
parameters retain SMALLINT. The oracle was stopped after these probes.

Jev selected a narrower mixed-anchor inference (receipt
`target/assembly-review/width-bucket-mixed-parameter-contract-*`). For a
builtin four-argument call after the existing declared-routine lookup, accept
at least one qualified non-parameter anchor and at least one fresh qualified,
nonliteral direct unknown operand parameter; require every anchor to be
uniformly FLOAT8 or uniformly NUMERIC. The count must be known INT4 or a fresh,
qualified, nonliteral direct unknown parameter in a distinct slot from the
unknown operands. Repeated operand slots are allowed. Apply existing occurrence
demands only to unknown parameters in argument order and retain INT4 result
metadata. Numeric typmods share the NUMERIC OID. If a count cast resolves an
earlier unknown operand slot to INT4 while its saved summary is still unknown,
the existing occurrence ledger reports the 42P08 INT4-versus-FLOAT8 conflict;
a parameter already resolved before the operand is inspected bypasses this
subset.

Mixed anchor types, direct already-known parameter anchors, SMALLINT counts,
shared unknown operand/count slots, no anchor, nonfresh and contextual NULL parameters, literals,
other overloads and #813 remain open; all continue along the previous paths.
Existing occurrence inference and allocation limits apply. Probes are authored
but unexecuted pending consolidated validation.
