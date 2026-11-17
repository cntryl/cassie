# Transaction authority runtime closeout

This record covers the finite Cassie-side requirements of #1290 and #1279.
It advances #1271 without claiming power-loss, real-provider cloud-loss or
bounded native/background quiescence. Published `cntryl-midge` 0.5.2 remains the
only storage layer; #1188 and cntryl/midge#1045 retain their independent API and
qualification boundaries.

## Source identities

| Evidence | Commit | Tree |
| --- | --- | --- |
| Base (merged qualification PR #1332) | `8f010a9d9bd7054198de7ee5e48161def707223d` | `c6c468eb1e7d3c5fc587c68b5b88ebf869316338` |
| Executed complete default suite and 34 feature controls | `843500f857926f3f1986942ae634912eb692bfd2` | `6e0239e44c5fb4cff48ee90ef954f031bc47803e` |
| Later affected checks and fresh SQL8 diagnostics | `168527ec5804b47511a3a662484984a935be5c11` | `b67f2cdf03fa6e01d6b6e8b28b9fe8157ce48d49` |

Between the two executed sources, production changes only flatten an equivalent
late-recovery conditional. Test changes add a semicolon to a unit-returning
store, express the same 1,024-byte limit in its initializer, and rename 17 test
identifiers. Exact-blob review preserves the unit value, limit, control flow and assertion
oracles of the syntax changes. The independent inverse-rename proof preserves
every body, attribute, assertion and selector in the name-only increment. The [rename map](rename-map.json)
retains the old names in the actual 843500 execution evidence; the
[body-equivalence probe](rename-body-equivalence.json) checks the rename increment. A fresh local full
suite was not executed at 168527; the coordinator selected affected-check repeats
under the assembly-line runbook. The delivery PR records final publication-head
hosted full-suite checks and merge readback separately.

Later owning-document and evidence closeout does not change Rust, manifests or
lockfiles from 168527. This committed record is local evidence; its existence is
not a substitute for the publication-head hosted checks.

## Observable behavior

- An old operation from transaction A cannot fail or retire replacement B through
  simple, extended or COPY completion, quarantine conversion, portal publication
  or terminal cleanup. Matching current-owner failures retain their existing
  failed-block behavior. A stale admitted operation returns 25000 before core
  entry; subsequent current-owner access still observes a latched 58030 fence.
- Sequence/default effects admit ownership and sticky strictness before native
  submission. A determinate adapter unique-conflict retry reselects policy after
  actual deferred effects; outcome-unknown does not retry. Real wire COPY controls
  observe the owning Data worker independently of aggregate counters.
- Detached forks retain private setting payload and LOCAL restoration history.
  Shared objects, clocks and schema state validate their actual root lineage.
  Worker leases, portal metadata and shared COPY reservations follow actual
  retained owners through cancellation, disconnect and last-clone release.

## Local validation

| Command | Actual source and result |
| --- | --- |
| `cargo build --locked --bin cassie` | 843500: passed, 15.28 s |
| `cargo test --locked` | 843500: 14,460 passed, zero failed, nine inherited ignores; 717.29 s including 4m39s compilation |
| Five feature-enabled module commands below | 843500: all 34 passed, expected counts checked, no ignored cases |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | 168527: passed, 7.11 s |
| `cargo fmt --all -- --check` | 168527: passed, 4.72 s |
| `cntryl-tools validate-tests -f <each touched test file>` | 168527: all 41 commands exited zero; 292 recognized tests compliant in 40 files; one shared fixture has no direct tests |
| Pinned Python `tools/test_inventory.py --check` | 168527: passed; 10,529 integration declarations, 10,554 registrations, no unresolved/unregistered/duplicate identities |
| Static source-unit manifest hash/declaration check | 168527: passed; 37 files, 263 declarations (103 added-in-file, 21 modified, 139 retained); not a runtime registration claim |
| Replay bench `--profile test --no-run` | 168527: passed, 138.83 s compilation, separate from lane measurements |
| Fresh eight-cell SQL diagnostic | 168527: all eight cells passed, 4,400 batch samples; 1,378.67 s total command time |

Each feature command was `cargo test --locked -p cassie --lib --features
replay-benchmark-observation <selector> -- --nocapture`:

| Selector | Executed cases |
| --- | ---: |
| `app::projection_sql_durability_tests::` | 11 |
| `app::replay_api_durability_tests::` | 8 |
| `app::session::sequence_effect_tests::` | 9 |
| `pgwire::connection::copy::native_policy_tests::` | 3 |
| `app::statement_mutation::retry_policy_tests::` | 3 |

The generated source-unit manifest retains static authored/not-run markers for
its current declaration identities. It does not evaluate parent cfg, infer
registration or relabel an earlier source run; the source-specific results and
equivalence evidence in this record are separate.

The six transaction-clock controls executed in the default suite. The fulltext
boundary regression requires a positive persisted-posting read, then actual
restricted executor scan progress before ResourceLimit, zero remaining tracker
bytes and no success/fallback increment. Its separate actual SQL request retains
its 1,024-byte rejection and release oracle. The observed SQL-before-loading
failure was corrected by this explicit two-boundary contract, not by claiming any
ResourceLimit proves persisted scoring occurred.

## Observed failures and authoring evidence

The [attempt index](gate-attempts.json), [original gate archive](gate-receipts.tar.gz)
and [per-file SHA manifest](gate-receipts-manifest.json) preserve failed
build/test/strict/policy attempts separately from later passes.
They include ten original unit failures, two ANN fixture setup failures, three
REST setup failures, the fulltext setup and actual SQL-before-loading failures,
compiler diagnostics, strict lint findings and the 17-name policy failure.
Historical remaining-binary diagnostics retain executable SHA and matching Cargo
library/support fingerprints; they do not substitute for the later full test7 pass.

Core, sequence, terminal and resource regressions were authored before their
implementation increments as recorded in the archive’s explicitly historical
handoffs. Their authored/not-run statuses describe those handoff times, not the
current gate result. Outer-wrapper coverage
was authored after the initial wrapper code. Those authored tests were unexecuted
at source handoff; no instrumented baseline RED is invented. Actual integrated
failures and subsequent executable passes are identified by their real commits.

## Review and probe interpretation

The [acceptance map](acceptance-map.md) links issue criteria to source and actual
probes. The [typed question ledger](question-ledger.json) retains exact questions,
criteria, typed answers, probabilities and request/response hashes, with source
payloads and authentication excluded. The [lead/action record](probe-dispositions.json) distinguishes fixed,
confirmed, disproved, executed and external-blocked obligations. Jev is advisory;
question counts and model probabilities are not proof or executable qualification.

The unique-retry witness observes a real typed **adapter** UniqueViolation before
owning Data submission, then actual deferred sequence effects and strict retry.
It does not claim a native Engine commit returned uniqueness failure. The unknown
control observes native Data success followed by the adapter response seam;
quarantine and absence of resolver/retry are tested, not native fault or power loss.
Simulated-cloud policy selection is distinct from upload/cache-loss qualification.

## Fresh SQL diagnostic

See [the eight-cell summary](sql8/summary.json), [lane table](sql8/lanes.csv), all
individual samples in `sql8/raw-results.json.gz`, and the original per-lane logs,
JSON and execution receipt in `sql8/raw-receipts.tar.gz` with its SHA manifest.

Each cell uses one fresh local 100,000-event store: default/replayable embedded
SQL, indexed/unindexed, and 1,000 × 100 or 100 × 1,000. Commands explicitly select
test profile, `replay-benchmark-observation`, 64 MiB query memory and a 120-second
**per-query** guard. The command is `cargo bench --locked --features
replay-benchmark-observation --bench tier3_replay_durability --profile test` with
exact lane selectors in the raw receipt. This is embedded `execute_sql`, not
pgwire transport or client-latency measurement. Actual wire policy is covered by
the separate three functional controls.

Acknowledgement samples cover each actual batch; p50 is the median and p95 uses
nearest rank over individual samples. Setup, initial full verification, clean
reopen and reopened verification are separate phases. Every SQL transaction Result must succeed; each store then checks all rows,
16 exact title buckets, checkpoint and clean-reopen state. SQL does not return
replay-API applied/skipped reports; raw applied-event fields are driver workload
metadata, while the final-state checks are observed assertions.
An indexed fixture creates its scalar index; this writer diagnostic does not
independently force or attest the read planner's use of it.

Primary counters mean attempts outside explicit maintenance scopes, not uniquely
the owning Data commit. First policy is the first caller-thread observation.
Maintenance barriers count strict attempts inside maintenance scopes. Engine-wide
counters can include background work and omit unaccounted filesystem syncs.
`wal_accounted_syncs` already includes successful WAL fsync plus ledger file and
directory syncs; fields remain separate and are never added together.

One fresh store per cell is an affected-path diagnostic, not an independent-store
variance or speedup study. The earlier 48-control source58e6c96e evidence remains
historical. Three matched pairs had equal WAL-fsync/accounted-sync deltas. The unindexed
100 × 1,000 pair observed 10,011/20,039 for default and 9,011/18,037 for replayable;
both recorded 7,005 maintenance barriers. These are per-cell observations, not
a universal gain or no-gain conclusion. Clean reopen is not process-crash, cloud-loss or
power-loss qualification. No native completion deadline, real-provider durability
or universal throughput improvement is asserted by this record.

## Historical A derived-evidence clarification

The [late correction record](historical-a-sql-summary-correction.json) preserves
before/after summary hashes for sixteen SQL lanes in the earlier A apply report.
Only derived verification descriptions changed: SQL transaction success/final-state
checks are distinct from API applied/skipped reports. The other sixteen A apply
lanes and all sixteen API auxiliary lanes retain their report assertions. Original
raw bytes, source58e identities, measurements and typed review judgments remain
unchanged; the [independent source check](historical-a-sql-source-review.json)
records the confirmed branch distinction.
