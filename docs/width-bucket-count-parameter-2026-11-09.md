# WIDTH_BUCKET count parameter inference subset

PostgreSQL 18.6 resolves the unknown fourth parameter of four-argument
`width_bucket` as INT4 when the first three arguments are uniformly FLOAT8 or
uniformly NUMERIC; repeated uses retain that INT4 type, while a supplied
SMALLINT parameter remains SMALLINT. With four unknown parameters PostgreSQL
instead resolves FLOAT8/FLOAT8/FLOAT8/INT4. The oracle was stopped after
these probes.

Jev selected a narrower parameter-inference increment (receipt
`target/assembly-review/width-bucket-parameter-inference-contract-*`): after
the existing declared-routine check, apply the existing parameter-demand
machinery to an unknown fourth `Expr::Param` only when the builtin has exactly
four arguments and the first three admitted summaries are all known FLOAT8 or
all known NUMERIC. The resulting expression has INT4 result metadata. Supplied
parameter OIDs, other argument types and visible declared routines retain the
existing path. The existing occurrence ledger controls repeated parameter
inference and conflicting contexts; no new coercion mechanism is introduced.

Untyped string counts, all-unknown and mixed operand inference, complete
conflict precedence, general overload resolution and #813 remain open. The
SQL/wire probes are authored but unexecuted under the consolidated validation
deferral.
