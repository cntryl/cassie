# Projection replay profiles and finite read evidence

Cassie distinguishes a transaction's declared projection mode, the policy required
by its actual effects, and a commit's durability outcome. `SHOW
cassie.projection_write_mode` and `SELECT
current_setting('cassie.projection_write_mode')` report the declaration. They are
not a native policy attestation, upload acknowledgement, or durable-offset receipt.

Projectors may declare `BEGIN; SET LOCAL cassie.projection_write_mode =
'replayable'; ...; COMMIT`. Row and checkpoint updates belong in that transaction.
Keep the source available and implement the documented rewind/rebuild protocol
before independently acknowledging external offsets. Independent sequence or
other nonreplayable effects promote strictness; a declared `strict` transaction
retains strict policy. See [the SQL contract](sql-replayable-transaction-contract.md).
Cloud deployments select the documented
`CASSIE_STORAGE_CLOUD_DURABILITY=background|strict`; a declaration does not override
configured strict policy. Unknown outcomes require quarantine and recovery, not
blind retry.

Use `pg_projection_checkpoints` and the [source/checkpoint recovery contract](projection-replay-contracts.md)
workflow to inspect freshness and plan recovery. Embedded diagnostics expose
`Midge::native_durability_metrics()`; native per-policy observations additionally
require benchmark instrumentation. There is no documented pgwire/REST native-WAL
counter endpoint. Aggregate counters include background work and do not identify
one transaction's durability outcome.

## Finite read campaign contract

The selected local campaign contains **eight profiles**, one producer batch shape
of 1,000 rows × 100 batches, all with 100,000 rows: historical API strict/buffered
writers at `58e6c96ea14c3466da17c256cb2fcaefbd71f940`, and the separately recorded
final B SQL-default/replayable writer, each indexed and unindexed. Writer commit,
tree, successful producer exit/clean-close evidence, raw result/log SHA-256 and
reader commit/tree remain distinct. This does not rerun the writer matrices or
relabel historical write measurements as final-reader measurements.

Only recursively copied **closed stores** are opened. Before any copy, verify the
producer exit-zero/clean-close receipt and its exact writer identity, storage path,
raw result/log hashes, parameters and source-tree content fingerprint. Reject
missing/foreign/mismatched receipts and symlinks. Create a fresh owned temporary
directory; copy regular files by I/O, without hardlinks or symlinks. Verify the
copied content fingerprint and the original fingerprint after copying and after
the reader closes. Never open, delete a lease in, or write to an original store.
Lock-file absence is not closure proof. Negative controls exercise these ownership
and provenance boundaries through the same pure helpers used by the campaign.

The reader explicitly selects **64 MiB query memory**, a **120,000 ms per-query
deadline**, and disables execution-result caching per instance. These are diagnostic
profile choices; ordinary 10 MiB/30-second defaults are unchanged. Operators can
select the corresponding documented `CASSIE_QUERY_MEMORY_BUDGET_BYTES` and
`CASSIE_QUERY_TIMEOUT_MS` values when that explicit profile is appropriate; a
120-second query cap is not a whole-campaign deadline or a throughput promise.

One untimed round warms the sixteen equality-bucket queries and independent total
count. Three timed rounds then retain every SQL and elapsed-time sample: **48
equality samples and 3 count samples per profile, 408 timed queries total**.
Timing covers SQL execution and materialized results. Correctness checks, plans,
metrics, copying, fingerprinting and startup are outside those timers. Hashing,
copying and warmup affect caches; no cold-cache or physical-I/O isolation is claimed.
Family p50/p95 use individual samples; with only three count samples, count p95 is
the observed maximum, not a stable population estimate.

Each equality query must return exactly its complete 6,250-row bucket with the
expected non-null title, and the independent count must be exactly 100,000. Indexed
profiles must attest the named scalar index in the public plan with no fallback,
and actual per-query executor read-path counter/last-index evidence. Unindexed
profiles must attest scan execution. A planner or execution fallback is an explicit
failed/gap result, never silently accepted, forced through a hint, or repaired by
changing the planner. The sixteen disjoint buckets cover all rows without raising
the selected memory budget for an unbounded full-result materialization.

This contract and its probes are authored; no C read timing, index attestation, or
operator readiness result is established until the assembler runs and records the
frozen source. See the [benchmark entry](benchmarks.md) and [performance contracts](performance-contracts.md).

## Running the read-only-original campaign

The assembler prepares a JSON manifest with `format: "cassie-replay-read-v1"`,
full `reader`, `api_writer` and `sql_writer` objects (each `commit` and `tree`),
an explicit `reader_build_profile` label, and exactly eight `profiles`. The API
writer tree is `d051222b580d48266b3ed604f588928bd443e660`; SQL producer identities
must match their separately qualified B source. Each profile supplies these files
and hashes:

| Manifest profile field | Meaning |
| --- | --- |
| `result_path`, `result_sha256` | Original per-producer JSON result and exact SHA-256 |
| `log_path`, `log_sha256` | Retained producer subprocess log and exact SHA-256 |
| `closure_path`, `closure_sha256` | Caller-verified exit/clean-close receipt and its exact SHA-256 |
| `source_fingerprint` | Full closed-store relative-path/type/length/content fingerprint from the shared helper |

The closure receipt binds `writer`, `storage_path`, `result_sha256`,
`log_sha256`, `exit_code: 0` and `clean_closed: true`. It must derive from the
retained successful producer subprocess and its known clean-shutdown path;
setting a boolean is not a live-engine absence detector. The caller owns closure
and must prevent later openers. The tool checks this receipt chain and hashes,
then verifies original content around copying and reading. Fingerprints exclude
access times. Source/build labels require independent Git/build-receipt checks;
the executable does not cryptographically self-attest its build identity.

```sh
CASSIE_REPLAY_BENCH_READ_MANIFEST=/absolute/path/read-manifest.json cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability > read-campaign.jsonl
```

Record the actual command, build profile, host and source/tree in the campaign
receipt; do not label a `cargo bench` build as the historical test profile. Output
includes the manifest hash, each attempted warmup/timed SQL with correctness and
path evidence, every individual timing sample, and per-profile distributions.
A profile gap retains preceding raw records and makes the finite campaign fail.
No original is opened even on that path. The benchmark fixture/copy negative
controls live in `tests/bench_diagnostics/replay_read_ownership.rs`; assembler-owned
registration makes them part of the normal correctness gate.

Before constructing the manifest, the same reader executable can fingerprint a
closed source without constructing an engine or copying anything:

```sh
CASSIE_REPLAY_BENCH_SOURCE_FINGERPRINT=/absolute/closed/store cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability
```

Its JSON identifies `relative-path-type-length-sha256-v1` and the source path.
This utility only hashes regular files; it does not attest closure or provenance.

For historical API inputs, extract the four `1000x100-repeat1-{shape}-{mode}-apply`
`.json`/`.log` pairs from [the original A archive](evidence/replay120-2026-11-16/raw-receipts.tar.gz),
where `shape` is `indexed`/`unindexed` and `mode` is
`local_strict`/`local_buffered`. Verify the archive against its
[manifest](evidence/replay120-2026-11-16/raw-receipts-manifest.json) and retain
[the actual producer exit-zero receipt](evidence/replay120-2026-11-16/apply-matrix-exit.json)
and [source/build evidence](evidence/replay120-2026-11-16/README.md).
Use the original extracted JSON bytes when hashing; reserialization changes that
receipt hash. Obtain `storage_path` from each JSON. Independently establish that
these retained stores remain closed and exclusively unavailable to later openers
before deriving each bound closure receipt. The historical log's clean-reopen
claim alone cannot establish present closure.

For SQL inputs, use four newly qualified final-B producer `.json`/`.log` pairs,
`sql_default`/`sql_replayable` × `indexed`/`unindexed`, at the same batch shape.
Their exact exit receipts, writer SHA/tree and closure evidence remain pending;
do not substitute historical A SQL lanes or invent successful B closure records.
The manifest cannot run until those real receipts exist.

A preparation script can construct each profile from these real inputs using
`hashlib.sha256(Path(path).read_bytes()).hexdigest()` for its three receipt hashes.
The bound closure JSON has this schema (angle-bracket values must be replaced
from verified evidence, never submitted as literals):

```json
{"writer":{"commit":"<producer-40-hex-SHA>","tree":"<producer-40-hex-tree>"},
 "storage_path":"<original-result-storage_path>","exit_code":0,"clean_closed":true,
 "result_sha256":"<original-json-sha256>","log_sha256":"<original-log-sha256>"}
```

The resulting top-level manifest shape is:

```json
{"format":"cassie-replay-read-v1",
 "reader":{"commit":"<final-reader-40-hex-SHA>","tree":"<final-reader-40-hex-tree>"},
 "reader_build_profile":"<actual-reader-build-profile>",
 "api_writer":{"commit":"58e6c96ea14c3466da17c256cb2fcaefbd71f940","tree":"d051222b580d48266b3ed604f588928bd443e660"},
 "sql_writer":{"commit":"<verified-final-B-writer-SHA>","tree":"<verified-final-B-writer-tree>"},
 "profiles":[{"result_path":"<extracted-original-lane.json>","result_sha256":"<sha256>",
 "log_path":"<extracted-original-lane.log>","log_sha256":"<sha256>",
 "closure_path":"<caller-verified-bound-closure.json>","closure_sha256":"<sha256>",
 "source_fingerprint":"<same-reader-fingerprint-utility-output>"}]}
```

Expand `profiles` to the eight distinct mode/shape pairs above. Profile identity
comes from its hashed producer JSON, not array order; duplicate cells/stores and
missing/foreign closure chains are rejected before any source copy is made.
