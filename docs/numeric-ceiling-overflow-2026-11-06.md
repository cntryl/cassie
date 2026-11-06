# NUMERIC ceiling and floor carry under #813

PostgreSQL 18.6 returns `22003` when `ceil` of a positive NUMERIC with the
maximum 131,072 integer digits and a nonzero fractional part carries into a
131,073rd integer digit. The matching negative `floor` also returns `22003`.
The opposite rounding directions retain an in-range result. Oracle receipts
are retained in ignored `target/scalar-oracle/adversarial-and-numeric-range.json`.

The existing SQL executor delegates to public Rust helpers that return
`Numeric` and discard `add`/`subtract` overflow with a truncated fallback.
Jev selected preserving those existing public function signatures and routing
SQL through private checked helpers (probability 1.00). This avoids a breaking
Rust API change while making the PostgreSQL-wire SQL path report the format
error. The public unchecked helpers retain their existing fallback and cannot
express overflow; that limitation remains an explicit open #813 obligation.
Jev's public-helper gap probability was 0.92 and was confirmed in source;
the owner issue remains open. No persistent format or storage change is made.

The boundary unit and wire regressions were authored before implementation.
Their execution remains deferred until the assembled gate; no baseline RED or
passing result is claimed. The 131,072-digit input is the minimum value that
can cross this fixed numeric format limit, not a throughput or timing workload.
