# Relational qualification checkpoint — 2026-10-27

Issue [#761](https://github.com/cntryl/cassie/issues/761) remains open. Its named prerequisite [#760](https://github.com/cntryl/cassie/issues/760) was read back CLOSED. This checkpoint records bounded local evidence; it does not qualify the full relational target, PostgreSQL parity, or a release.

The recorded run used a dirty working tree based on `db028e02b850c05b5294abe28783b2ace799d59e`; that commit alone does not identify the tested source. The fixture records SHA-256 is `5b3f38f7c907227aa31fc3729c8fec281490252ca33879f064860123d5a96f2d`. The retained local `target/batched-runtime/seed-array-review/full-tests.log` SHA-256 is `978a1a5aac9d5680be072b28a5ef6d21972d901c180d369b31e26caf794f9cb5`. The complete `cargo test --locked --no-fail-fast` run exited 101: pgwire_extended 181 failures, sql_mutations 5, sql_queries 16, and storage_indexes 1. These outputs are local artifacts, not immutable hosted evidence. GitHub Actions remain disabled.

The full run passed `primary::should_match_independent_relational_truth_tables` and `primary::should_preserve_primary_wire_descriptors`. The former checks recorded rows and embedded type descriptors; the latter checks statement and portal descriptors in both requested formats. Their [recorded fixture](../../tests/support/relational_qualification/records.json) contains the following 22 bounded primary witnesses. Each is **executed selected-profile evidence; broader acceptance pending**, not a complete disposition of its invariant. The target additionally has 20 positive variants and other separate cases. The final focused `cargo test --locked --test relational_qualification` passed 79/79 after formatting and the seed-checker test name change.

| Invariant | Executed primary SQL |
| --- | --- |
| `SQL-001` | `SELECT id,label FROM r WHERE n >= 0 ORDER BY id` |
| `SQL-002` | `SELECT n FROM r GROUP BY GROUPING SETS (n)` |
| `SQL-003` | `SELECT "Foo",foo FROM case_fields ORDER BY id` |
| `SQL-004` | `SELECT n FROM triage.dot` |
| `SQL-005` | `SELECT l.n,h.n FROM scope_l AS l JOIN scope_h AS h ON l.id=h.id ORDER BY l.id` |
| `SQL-006` | `SELECT q.x,q.a FROM (SELECT n AS x,a FROM r WHERE id=3) AS q` |
| `SQL-009` | `SELECT n+1 AS x,n/2 AS half FROM integers ORDER BY id` |
| `SQL-010` | `SELECT id,CASE WHEN n=0 THEN 9 ELSE 10/n END AS x FROM guarded ORDER BY id` |
| `SQL-011` | `SELECT id,COALESCE(n,CAST(4 AS BIGINT)) AS x FROM r ORDER BY id` |
| `SQL-012` | `SELECT id FROM r ORDER BY n ASC,id` |
| `SQL-013` | `SELECT id FROM r WHERE n>=0 ORDER BY n,id LIMIT 2 OFFSET 1` |
| `SQL-014` | `SELECT label,n FROM r ORDER BY 2,1` |
| `SQL-015` | `SELECT DISTINCT n FROM r` |
| `SQL-016` | `SELECT n FROM sa UNION SELECT n FROM sb INTERSECT SELECT n FROM sc` |
| `SQL-017` | `SELECT COUNT(*) AS rows,COUNT(n) AS cells,SUM(n) AS total,MIN(n) AS low,MAX(n) AS high,AVG(n) AS mean FROM r` |
| `SQL-018` | `SELECT g,SUM(n) AS total FROM r GROUP BY g HAVING SUM(n)>6 ORDER BY g` |
| `SQL-019` | `SELECT l.id,h.id FROM l LEFT JOIN h ON l.k=h.k ORDER BY l.id,h.id` |
| `SQL-020` | `SELECT l.id,h.id FROM l JOIN h ON l.k=h.k ORDER BY l.id,h.id` |
| `SQL-021` | `SELECT o.id,q.n FROM r AS o JOIN LATERAL (SELECT i.n FROM r AS i WHERE i.id=o.id AND i.n>=0) AS q ON true ORDER BY o.id` |
| `SQL-022` | `SELECT o.id,EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id AND i.n>=0) AS present FROM r AS o ORDER BY o.id` |
| `SQL-024` | `WITH RECURSIVE seq(n) AS (SELECT n FROM seed UNION ALL SELECT n+1 AS n FROM seq WHERE n<2) SELECT n FROM seq ORDER BY n` |
| `SQL-025` | `SELECT id,RANK() OVER (PARTITION BY g ORDER BY n) AS rank,DENSE_RANK() OVER (PARTITION BY g ORDER BY n) AS dense FROM r ORDER BY id` |

## Remaining gates and review

- Prior independent PostgreSQL preparation retained 48 passing and 18 failing cells, with explicit seed OID adaptations. Issue #761 identifies unresolved comparison obligations for array OIDs, result labels, BIGINT aggregate identities, and mixed numeric joins. The current 79-case target does not replace that comparison.
- The full run still fails the array ANY/ALL NULL/empty witness (`op ANY/ALL (array) requires array on right side`), the multidimensional TEXT[] constructor witness (`0A000` before JSON execution), and ordered JSON aggregate input (`json_agg is not an aggregate function`). Their respective dependency owners [#762](https://github.com/cntryl/cassie/issues/762), [#807](https://github.com/cntryl/cassie/issues/807), and [#811](https://github.com/cntryl/cassie/issues/811) remain open. These are failure evidence, not exclusions or closure evidence.
- SQL-017 row versus summary witnesses for mixed and all-NULL inputs, and selected parallel aggregate witnesses, passed in the full run. A subsequent focused `cargo test --locked --test analytics should_preserve_aggregate_results_across_column_segment_boundaries -- --nocapture` passed 1/1. It compares three mixed-NULL rows across segment sizes 1, 2, and 3, asserts exact values and OIDs, and requires the summary scan counter to advance. Jev's result-type oracle gap probability was 0.34; the test was refined with literal OIDs before execution. These cases do not complete every worker and batch boundary, and the full run predates this new test.
- Strict full pedantic Clippy failed with 57 findings in the accumulated tree; workspace format check failed. These are outstanding ordered-validation gates. The selected fixture test-policy check passed 1/1.
- Jev assessed the probability of an unresolved #761 acceptance gap at 0.80 and selected bounded selected-profile witnesses at probability 0.90. The failing cases and uncompleted path/oracle gates above are the focused follow-up; the judgment is advisory and does not prove a gap or completion independently.

Next: resolve or document each selected independent-oracle and execution-path obligation, retain exact final-source evidence, rerun the full ordered gate, and keep #761 open until every criterion is satisfied. Do not promote #762/#807 merely because the primary witness table is green.

## Segment-sweep validation

After the segment-sweep test and inventory update, `cargo build --locked --bin cassie` passed and `cargo test --locked --no-fail-fast` completed with 202 failures: pgwire_extended 180, sql_mutations 5, sql_queries 16 and storage_indexes 1. The log is retained locally at `target/batched-runtime/issue761-evidence/full-tests.log` (SHA-256 `faf4df074fa26c0741100d13746b002e9f64b713352b01c32c01d82190b9d079`). The previous 203-failure run had one additional failure: a numeric digit-limit wire test timed out then passed in this run. This documentation/test slice did not change that code, so the difference is run-to-run variability, not a claimed fix. No new failing identities appeared.

Full strict pedantic Clippy failed with 57 findings in the accumulated source tree; workspace format check failed. The touched analytics test file passed `cntryl-tools validate-tests` 7/7, and `tools/test_inventory.py --check` passed using the local inventory environment. Neither the red full suite nor the failed lint/format gates qualifies #761.

## Join batch-size sweep

The focused `cargo test --locked --test relational_qualification should_preserve_homogeneous_join_results_across_batch_sizes -- --nocapture` passed 1/1. Using the existing five-probe/four-build-row literal fixtures, it checks inner and left joins at vectorized batch sizes 1, 2, 3 and 5. It asserts ordered rows, column count, names, OIDs and modifiers, selected strategy, build/probe counts and output counts. Jev assigned a 0.70 probability to an oracle gap; the follow-up added explicit descriptor count, format and names from the recorded fixture before the passing run. This finite homogeneous sweep does not close mixed-type, visibility, resource, or full #761 acceptance.

The subsequent ordered build passed and the full `cargo test --locked --no-fail-fast` run exited 101 with the same 202 failing identities as the preceding run and no additions. Its retained local log is `target/batched-runtime/issue761-join-sweep/full-tests.log` (SHA-256 `ae6893f99b3f7d82fcbbaaefe71b19845443ff2cf5a8af93d85c99a799982058`). Full strict pedantic Clippy again failed with 57 findings and the workspace format check failed. The two touched test files passed test-policy validation, and the generated inventory check passed. These failures keep the full ordered gate and #761 open.
