# Finite replay recovery and read evidence

The simulated-cloud controls recovered strict API/SQL acknowledgements at position **2** and replayable API/SQL prefixes at **1**, then reconciled all four to position 2 with row/index/checkpoint and retry assertions. The corrected closed-store read campaign passed **eight profiles, 408 actual timed queries and 136 actual warmups**. These are finite local diagnostics; [#1271](https://github.com/cntryl/cassie/issues/1271) and [#1188](https://github.com/cntryl/cassie/issues/1188) remain open for their broader requirements.

## Exact source and qualification

| Evidence | Executed source | Actual outcome |
| --- | --- | --- |
| Locked build/full default suite | `40a08ce25972bc3118d5dcc5dd9fb8f4c668cee9`, tree `9bff39744534ef6a125c6992fee2b5d2aa2ea168` | Build passed; 14,478 passed / 0 failed / 9 inherited ignores across 48 harnesses, 531.603 s command time |
| Focused recovery/process/ownership | Same `40a08ce2` | Cloud 9, legacy process 5 and ownership 9 passed. Cloud command 323.84 s includes 217.78 s compilation; cloud harness 106.06 s. Four printed recovered positions are retained |
| Corrected metadata and ownership helpers | `a5fd91760e8cab27dad04e2d60b019b446304d4a`, tree `c66c6a32199c64fcd9cbeebff9b57a2922530bee` | New metadata 4 and existing ownership 9 passed, zero ignored |
| Affected standing gates | Same `a5fd9176` | Full strict Clippy, fmt, all eight touched-file policy commands (36 recognized controls), integration inventory 10,542 declarations / 10,567 registrations and unit inventory 5 files / 23 declarations passed |
| Corrected read campaign | Same `a5fd9176` | Exit 0, 1,596.667719 s whole command; eight passed profiles, 408 timed / 136 warmup samples |

The intervening source changes are reviewed benchmark/helper repairs; production runtime source is unchanged after `40a08ce2`. The long default/cloud results are not relabelled as executions of `a5fd9176`. Final publication-head hosted full-suite checks are separate and must pass before merge. Unit inventory records are static authored-source snapshots; executable results are mapped here and in the acceptance record.

Actual commands are retained verbatim in the raw archive: `cargo build --locked --bin cassie`; `cargo test --locked`; focused `cargo test --locked -p cassie --lib <cloud/process module> -- --nocapture`; `cargo test --locked -p cassie-integration-tests --test bench_diagnostics <ownership/metadata module> -- --nocapture`; `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`; `cargo fmt --all -- --check`; `cntryl-tools validate-tests -f <each touched test file>`; pinned `tools/test_inventory.py --check` and the unit manifest checker.

Reader compilation used `cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability --profile test --no-run`. The actual campaign then executed that exact native artifact directly with `CASSIE_REPLAY_BENCH_READ_MANIFEST` set to the retained manifest, with no compiler running. The [build identity](reader-build-identity.json) binds command, source, executable and Cargo fingerprint hashes. Source/build labels are trusted local provenance, not cryptographic self-attestation.

## Read observations

Every profile uses 100,000 rows, writer shape 1,000 × 100, 64 MiB query memory, a 120,000 ms per-query deadline and disabled result caching. Each has 17 untimed warmups, 48 equality samples and 3 COUNT samples. Medians and nearest-rank p95 are recomputed from [all individual samples](read-samples.csv), not cumulative histograms or the planned header. COUNT p95 is the maximum of only three observations.

| Mode | Shape | Equality p50 ms | Equality p95 ms | COUNT p50 ms | COUNT p95 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| local_strict | indexed | 21.894 | 32.498 | 4834.643 | 4860.370 |
| local_buffered | indexed | 21.272 | 33.077 | 4903.594 | 4966.357 |
| local_strict | unindexed | 5013.134 | 5029.277 | 4924.840 | 4948.369 |
| local_buffered | unindexed | 5015.971 | 5039.646 | 4934.008 | 4941.684 |
| sql_default | indexed | 23.009 | 33.408 | 6656.445 | 6667.218 |
| sql_replayable | indexed | 23.396 | 33.559 | 6673.162 | 6675.156 |
| sql_default | unindexed | 5230.955 | 5253.998 | 5143.507 | 5158.662 |
| sql_replayable | unindexed | 5214.406 | 5234.535 | 5121.820 | 5131.977 |

The [machine summary](read-summary.json) retains exact nanoseconds. Every equality returns its complete 6,250-row bucket with distinct IDs and expected title; each warmup/timed round covers exactly 100,000 distinct IDs, with an independent COUNT of 100,000 and checkpoint validation. Indexed equality attests public `replay_bench_title_idx`, no fallback, actual native index operations and zero collection scans. Unindexed equality attests actual collection scans and no index operations. Raw catalog/native definitions and separately validated positive native relation/index identities are retained. All eight original fingerprints match after copying and after reader drop.

Only SQL execution/materialization is timed. Copying, hashing, startup, plans, metrics, validation and ID bookkeeping are outside timers; they can affect caches. These observations establish neither cold-cache isolation nor across-host variance, general speedup, pgwire latency, provider/power-loss durability or native quiescence. Originals were never opened as engines; each reader used a distinct byte-copied owned directory. Reader drop is a graceful-close attempt, not a global WAL-quiescence receipt.

## Inputs and preserved failed evidence

Four B SQL stores retain their actual writer `168527ec5804b47511a3a662484984a935be5c11` / tree `b67f2cdf03fa6e01d6b6e8b28b9fe8157ce48d49` raw/log/exit chains. Historical A API temporary stores were absent (including repeat-two alternatives); their original measurement records remain unchanged. Four **NEW** replacement API inputs were generated from the independently verified retained exact `58e6c96e` / tree `d051222b580d48266b3ed604f588928bd443e660` executable. Their fresh generation, raw/log/exit receipts, directories and fingerprints are separate from all historical A measurements. No original A receipt is attached to a recreated dataset; no full writer matrix was rerun. See [fresh producer results](fresh-api-input-producers.json), [availability census](historical-api-store-availability.json) and the [input identity map](writer-identity-map.json).

The first reader at `66916fee39adc277b27540366200bfa013113110` / tree `3308fca2f53958e28b37b6a66d7751b376a5832e` **failed**, exit 101 in 1,446.331443 s: four unindexed profiles passed 204 timed / 68 warmup queries; four indexed profiles failed before querying because the benchmark compared raw native storage-ID options to hydrated logical definitions. This is not an eight-profile/408-query success. Its [separate summary](failed-first-read-summary.json), original manifest, raw gaps and valid partial samples are preserved.

After that campaign ended, the exact reviewed standalone public-API diagnostic opened four new owned copies. It observed the same logical fields/catalog OID in each, native relation ID 12/index storage ID 16, matching collection IDs 12, and exactly the two intentionally hydration-cleared ID options absent from catalog definitions. All original fingerprints remained unchanged. The subsequent doc-first benchmark-only repair strips exactly those two keys on a clone for logical comparison and separately rejects missing/invalid/nonmatching native identities; every other field/option and the actual query/path/row oracles remain intact. Four pure regressions passed before the complete corrected eight-profile campaign. The failed first campaign is retained, rather than replaced by the passing result.

Initial constructor-visibility/SHA-format/type-inference compiler failures and strict-lint failures are preserved as failed attempts. They are not observed runtime regression RED. New controls were authored before their selected implementation increments where recorded; unexecuted baselines were never claimed to fail. The actual metadata-comparison failure and standalone diagnostic provide the observed oracle repair evidence. No check was narrowed, suppressed or turned into a pass.

## Durable records and remaining limits

The [archive manifest](archive-manifest.json) binds every raw receipt, exact command/time/source/status, producer input chain, fingerprint, sample, failed attempt and derivation script in [the compressed raw archive](gate-and-campaign-raw.tar.gz). Dataset files, executable/library artifacts, credentials, auth headers and large Jev source-state payloads are excluded. Exact executable/dependency hashes remain in the receipts. The [acceptance map](batch-c-acceptance-map.md) and [structured question ledger](batch-c-question-ledger.json) retain seventeen review receipt pairs / 48 exact typed questions, results, action mappings and hashes. Their judgments are advisory and distinct from executed probes; actual local outcomes are mapped separately. The [focused local-link probe](doc-link-probe.json) checks changed Markdown files and policy redirect callers, with no new/current broken links. External URLs are excluded from that focused probe.

Power-loss facilities, real cloud-provider/cache-loss qualification and the published native API blocker remain outside this finite campaign. No CI/release/version configuration or upstream engine source changed. These results advance #1271 without closing #1271 or #1188, and do not establish production readiness.

Raw archive: SHA-256 `3740b9b56599b5b25403372687ec98c3877f8fb62d685e46471fe4eb0ec69655`, 475,451 bytes, 167 members. Corrected JSONL: SHA-256 `a214baa9020ab9a497c3de4adeaaf968c2f8d04d04c6b6da050872e37ec2bd86`.
