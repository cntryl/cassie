# Organization validation

Recorded on 2026-10-15 on `feature/postgres-18-compat`, working from `f4919658` plus the existing uncommitted test backfill and this organization pass. There is one checkout/worktree. These results qualify the inventory and the specific moves; they do not qualify the full PostgreSQL compatibility backlog.

## Checks

| Command or probe | Result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed |
| `cargo test --locked --test backend_ci` | Passed: 2 tests; all 32 Cargo integration targets match exactly one CI shard |
| `cargo test --locked --test pgwire_extended --test compatibility --no-run` | Passed: both owning suites compiled |
| `cargo test --locked --test sql_queries identifier_case_resolution:: -- --nocapture` | Passed: all 16 moved tests, under the same registered module names |
| Compiled test binaries with `--list` | Exact match with the static inventory: `sql_queries` 1,149; `compatibility` 663; `pgwire_extended` 1,082; `backend_ci` 2 |
| Independent file-set and attribute census | Passed: all 673 files represented; 5,263 explicit Rust test attributes, 16 Python test declarations, unique source IDs and no registration/path/shard anomalies |
| `target/test-inventory-tools312/bin/python tools/test_inventory.py --check` | Passed: generated inventory matches current source |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Failed; a repeat with JSON diagnostics recorded 176 findings in 112 accumulated test files. No remaining findings in the nine organization files checked |
| `cargo fmt --all -- --check` | Failed: existing formatting differences across the tree and rustfmt's failure to resolve the inline compatibility module's Prisma child path. The normal compiler resolves that path and the compatibility suite compiles |
| `rustfmt --edition 2021 --check --config skip_children=true <organization Rust files>` | Passed for all nine organization files after formatting the touched suite root. This focused check does not replace the failed full formatter gate |
| `cntryl-tools validate-tests -f <organization Rust file>` | Passed for all nine files: 334 compliant declarations; two registration hubs contain no tests |
| `git diff --check` | Passed |
| Full `cargo test --locked` | Not run in this organization pass. The branch intentionally contains failing compatibility tests |

The Clippy findings include oversized test functions, empty-result assertions, documentation formatting and helper cleanup. They are defects in the accumulated suite and remain cleanup work. No lint suppressions or reductions were added. Runtime source, Cargo configuration and CI configuration were not modified.

## Newly reachable and relocated failing tests

| Test | Observed stopping point | Acceptance limit |
| --- | --- | --- |
| `row_arity_aliases::should_select_mixed_type_null_values_through_explicit_column_aliases` | Positive row-constructor control returns SQLSTATE `42601` with an unresolved `1::int4` column reference | The intended VALUES alias/mixed-NULL assertions are not reached |
| `row_arity_aliases::should_reject_row_comparisons_with_unequal_arity_using_postgres_syntax_error` | Equal-arity positive control returns the same `42601` error | Does not yet prove the unequal-arity rejection path |
| `compatibility::lifecycle::session_transaction::isolation_commands::repeatable_read_row_lock_conflict::should_abort_repeatable_read_row_lock_after_concurrent_update` | Setup returns `0A000`: only READ COMMITTED isolation is supported | Repeatable-read conflict and session-recovery assertions are not reached |
| `compatibility::lifecycle::large_objects::restart::should_restore_large_object_payload_after_storage_restart` | Setup rejects `lo_from_bytea` as unsupported | The persistence/restart assertion is not reached |

All four tests fail. Their failures demonstrate missing prerequisites and must not be counted as proof of the later acceptance paths. Their expected values and SQLSTATE contracts were retained; no runtime fixes were attempted.

## Review and remaining work

Three independent source reviews checked query, type/catalog/aggregate and lifecycle/support placement. Their concrete findings corrected false keyword matches, including percentile versus NTILE, ordinary “every” wording versus `EVERY()`, type identity versus sequences, string functions versus grammar, and client traces versus PostgreSQL JSON behavior.

Jev's initial multi-claim overstatement probability was 0.49. The follow-up probe independently reconciled files, source attributes, unique IDs, registrations and the compiled name lists. Jev identified “all tests backfilled” as the unsupported claim (0.99 choice confidence); the narrowed census/provisional-placement statement had a 0.25 gap probability. The guide therefore explicitly distinguishes a mechanical census, proposed destinations, unfinished physical migration and unfinished semantic deduplication. Jev is advisory; the source and command results above are the evidence.

Remaining organization work is enumerated in [the summary](summary.md) and [the per-file destinations](files.md). The 138 proposed names need acceptance-level deduplication against existing assertions before being treated as additional missing tests. Existing tests also require their own RED/GREEN evidence; this inventory does not assign runtime outcomes to unexecuted declarations.
