# WIDTH_BUCKET direct unknown parameters

PostgreSQL 18.6 infers FLOAT8, FLOAT8, FLOAT8 and INT4 for the four
unknown parameters in `width_bucket($1,$2,$3,$4)`. Repeating a parameter
among the first three operands retains FLOAT8; reusing one of those slots for
the count produces 42P08. A later explicit text cast of the first parameter
preserves its previously inferred FLOAT8 type. The oracle was stopped after the
probes.

Jev selected a limited direct-parameter increment (receipt
`target/assembly-review/width-bucket-all-unknown-contract-*`). After the
existing declared-routine check, when all four arguments are direct unknown
parameters and the count slot is distinct from all operand slots, the existing
occurrence ledger receives ordered FLOAT8/FLOAT8/FLOAT8/INT4 demands and the
result has INT4 metadata. Repeated operand slots are permitted. Explicitly
supplied or already resolved parameter types and all other shapes use the
previous path. A declared routine continues to take precedence.

Count slots shared with operands, mixed known and unknown arguments, literals,
pre-existing incompatible or later conflicting contexts, full overload
resolution and #813 remain open. The parameter occurrence machinery's
existing inference, admission and diagnostic limits still apply. Tests are
authored but unexecuted under the consolidated validation deferral.
