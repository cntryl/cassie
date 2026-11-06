# NUMERIC ceiling and floor carry under #813

PostgreSQL 18.6 returns `22003` when `ceil` of a positive NUMERIC with the
maximum 131,072 integer digits and a nonzero fractional part carries into a
131,073rd integer digit. The matching negative `floor` also returns `22003`.
The opposite rounding directions retain an in-range result. Oracle receipts
are retained in ignored `target/scalar-oracle/adversarial-and-numeric-range.json`.

The original public Rust helpers returned `Numeric` and discarded
`add`/`subtract` overflow with a truncated fallback. An initial Jev choice
favored a private SQL-only checked path, but its 0.92 probability of retaining
a public-helper defect was confirmed by source and the native boundary probe.
Given the user's direction to complete the code, the decision was revisited:
Jev selected changing both public helpers to return
`Result<Numeric, TypeInputError>` (0.99), with a source-compatibility impact
probability of 0.96. This is an intentional Rust source API break for callers
of `numeric::math::{ceil,floor}`: they must handle the result. There are no
other in-repository callers beyond the SQL executor. SQL and direct callers
both receive `22003` on a carry beyond the numeric format. NaN, infinities,
and in-range opposite-direction rounding remain unchanged. The decision and
probabilities are advisory; source, the PostgreSQL 18.6 oracle, and the final
executable gates determine acceptance. No persistent format or storage change
is made.

The boundary unit and wire regressions were authored before implementation.
Their execution remains deferred until the assembled gate; no baseline RED or
passing result is claimed. The 131,072-digit input is the minimum value that
can cross this fixed numeric format limit, not a throughput or timing workload.
