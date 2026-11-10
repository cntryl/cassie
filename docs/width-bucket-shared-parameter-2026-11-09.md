# WIDTH_BUCKET shared parameter conflict

PostgreSQL 18.6 rejects `width_bucket($1,$2,$3,$1)` during Parse with 42P08:
the first operand requires FLOAT8 while the count requires INT4. Repeated uses
among the first three operands themselves are valid. The oracle was stopped
after observing these cases.

For four direct, qualified unknown parameters, extend the existing ordered
FLOAT8/FLOAT8/FLOAT8/INT4 inference to a shared count slot only when every
participating occurrence is nonliteral and has no prior legacy inference.
Check all occurrence flags before mutating the ledger. The existing occurrence
coercion then produces its `Inconsistent` 42P08 diagnostic, including the
FLOAT8 versus INT4 detail, and the failed Fold retires its state. Disjoint
slots retain the preceding behavior. Embedded contextual NULL and any prior
legacy inference continue down the preceding path; neither is qualified by
this change. Supplied or already resolved types, mixed arguments, broader
conflicting contexts and #813 remain open.

SQL/wire and source-gate probes are authored but unexecuted pending the
consolidated validation gates.
