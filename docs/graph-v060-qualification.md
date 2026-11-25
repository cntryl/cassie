# v0.6.0 graph qualification

Qualification recorded on 2026-11-16: **correctness checks passed; timing limits remain**. The
finite mapped-graph and native-path contract has implementation and qualification
evidence below. Local correctness and client qualification do not establish
production capacity, an SLA, SQL/PGQ conformance, a release publication, or a
trustworthy performance regression baseline. Cassie remains a pre-release project.

## Source and local checks

The baseline is `9d25356a676614027172749644719b398a24a179`. This closeout changes
qualification fixtures, benchmarks, tests and documentation; production Rust
source under `src/` is unchanged. Midge remains the direct published 0.5.2 storage
dependency. Package version and CI/release configuration are unchanged.

| Check | Exact source | Observed result |
| --- | --- | --- |
| `cargo build --locked --bin cassie` | `0fffb6d0` | Passed |
| `cargo test --locked` | `0fffb6d0` | 14,358 passed, 0 failed, 9 existing ignored; 48 target/doc summaries |
| `cargo build --locked --bin cassie` and `cargo test --locked --test graph_types` | `838095f3` | Passed; 106 graph-type tests, including the two new reference/clone witnesses |
| `cargo build --locked --bin cassie` and `cargo test --locked --test bench_diagnostics` | `051af760` | Passed; 15 benchmark diagnostics tests |
| Full workspace/all-target/all-feature strict pedantic Clippy | `051af760` | Passed |
| `cargo fmt --all -- --check` | `051af760` | Passed |
| `cntryl-tools validate-tests -f <path>` for every changed Rust test file | `051af760` | All five files passed |
| `target/qualification-venv/bin/python tools/test_inventory.py --check` | `051af760` | Passed |

The complete suite was run before four focused tests were added. The affected
suites above execute those additions; the earlier full-suite result is not
relabeled as a later-head run. Final-head hosted results belong to the closing
pull request. Ignored tests are existing opt-in external-client/operational
fixtures, not graph tests added or skipped by this change.

An optional whole-document link audit (`cntryl-tools validate-docs --root docs`)
reports eight inherited links outside this graph slice: two historical anchors
in `feature-support.md`/`type-contract.md` and six ignored-target evidence links
in `array-aggregate-contract-2026-11-11.md`. Their link text is present in the
baseline; this check is failed, not counted as passed. The owned graph-oracle
link was corrected to its concrete `mod.rs` file.

Exact strict Clippy command:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
```

Touched test validators cover `tests/bench_diagnostics.rs`,
`tests/bench_diagnostics/graph_topology.rs`, `tests/graph_table/fast_reads.rs`,
`tests/graph_types/images.rs` and `tests/graph_types/refs.rs`. Detailed local
command/time/tree receipts remain in
`target/assembly-line/graph-qualification/{gates,final-gates,repaired-topology-gates}`.

## Native paths and real clients (#1268)

Psycopg 3.2.13 (Python 3.12.13, libpq 17.6) passed all five invariants on
`9b3aabae` using the optimized feature-enabled fixture. The
[client receipt](evidence/graph-v060/psycopg.json) records the actual command and
output. It covers executor-emitted one-hop and zero-hop native paths, prepared
text and binary parameters/results, path arrays, Describe OIDs, exact bigint
extrema and UUID keys, nested escaping, typed key/step relations, and malformed
input followed by successful reuse. The first attempt failed in catalog setup
with zero tests executed; the two metadata-name parameters now use explicit
`::text` casts. The corresponding `tokio-postgres` native-driver owners passed
in the complete and affected graph-type suites.

The selected public Psycopg workflow reads type OIDs, array OIDs and ordered
field types from Cassie's catalog and registers `CompositeInfo` in dependency
order. It uses Psycopg's own record/array adapters; no private adapter or custom
wire codec is supplied. This qualifies that explicit metadata-registration
workflow, not `CompositeInfo.fetch()` or arbitrary PostgreSQL catalog parity.
SQLx's builtin scalar/JSON graph workflow remains qualified; SQLx native
composite resolution is still unsupported under the documented catalog boundary.

Identity policy is clone-only: an image creates a distinct database identity
and omits graph definitions. Recreating the same graph name never retargets a
source reference; resolution returns `42704`. Identity-preserving restore and
automatic/explicit remapping are not offered. A reference has no row generation:
deleting and reinserting the same typed key resolves to the replacement row.
The new direct witnesses are
`refs::should_resolve_an_old_reference_to_the_replacement_after_reinserting_its_key`
and
`images::should_reject_a_copied_source_reference_after_recreating_the_graph_in_a_clone`;
both passed in the 106-test graph-type suite.

## Benchmarks and measured limits (#1260)

The raw artifacts, sample-level values, source identities, host/toolchain,
commands, exit codes and SHA-256 hashes are retained under
[`evidence/graph-v060`](evidence/graph-v060/manifest.json). They are local
diagnostics from an Apple M5 arm64 workstation with 24 GiB RAM and Rust 1.100.0.
The three release-profile owners each use five measured samples and one warmup;
the topology smoke uses one measured sample per point. No comparison baseline
is claimed. These are separate owner runs, not a complete single-head release
suite.

| Owner | Rows | Source | Correctness | Command / timing quality |
| --- | ---: | --- | --- | --- |
| Tier 1 native key/path kernels | 3 | `68179fdc` | All passed | Exit 0 via the diagnostic-only owner exception; all three timing rows failed quality |
| Tier 2 graph binding/algorithms | 4 | `68179fdc` | All passed | Exit 101 `QualityFailed`; one noisy timing row |
| Tier 3 read-model representatives | 4 | `68179fdc` | All passed | Exit 101 `QualityFailed`; three noisy timing rows |
| Tier 5 degree/depth/fanout smoke | 9 | `051af760` | All passed | Exit 0; diagnostic smoke, not scaling or capacity qualification |

The Tier 3 weighted route representative actually ran over 100,000 mapped rows
with a 64 MiB query-memory budget, returned three paths, and used indexed search
with no fallback. The separate native storage budget is 2 GiB with a 512 MiB
memtable; the 64 MiB figure is not a process-memory cap. Its five measured samples passed correctness and resources,
while timing remained noisy. Weighted search still performs the documented
O(selected edges) validation scan before neighborhood expansion.

These p95 observations are diagnostics, not gate-quality latency promises.

| Representative | p95 per query (µs) | Peak accounted query bytes | Timing quality |
| --- | ---: | ---: | --- |
| `graph_authorization_reachability_query/100k` | 304.412 | 18,960 | acceptable |
| `graph_hierarchy_ancestry_query/100k` | 1,874.533 | 58,433 | noisy |
| `graph_friends_of_friends_query/100k` | 2,358.134 | 58,433 | noisy |
| `graph_weighted_routes_query/100k` | 1,908,764.210 | 112,442 | noisy |

The generic memory-mode `storage_reads` observation is zero and does not
establish physical I/O or the neighborhood bound. Counted-bound evidence comes
from the focused indexed/fallback graph-read tests, not that generic counter.

The nine topology points and their registered profile were also accepted by the
shared `summarize_graph_scaling` example. The original smoke supplied observed
RED evidence: numeric scale `2` collided between the independent depth and
fanout fixtures. Axis-qualified scales (`degree-*`, `depth-*`, `fanout-*`) fix
the actual collision while leaving the shared fixture-identity tracker
byte-identical. The direct nine-key tracker regression and the repaired
all-nine smoke passed. Two observed Clippy integration failures (fixture config
initialization; duplicate module/dead imports in the tracker probe) were repaired
and the full strict invocation passed; no lint was silenced.

## Deterministic counter evidence (#1355, partial)

This section records counters only. No timing was measured or asserted, and no p95 result is claimed. The
bench owners `tier5_scaling_graph` and `tier3_system_graph` now write one record per measured case to
`target/graph-evidence/<owner>.json` (override with `CASSIE_BENCH_GRAPH_EVIDENCE_DIR`) beside their timing
output. Each case runs on its own freshly opened memory-mode database under the 64 MiB query budget with
result caching disabled. The first query after open is the cold run; the second is the warm run. The
indexed answer and the exact full read (`set_graph_exact_reads`, where it exists) are both compared with an
oracle computed in plain Rust over the generated edges in the bench harness (`benches/support/graph_*.rs`).
Both owners ran with `--profile smoke` (one sample, no warmup).

Counter columns: validated edges (weighted validation scan), expanded vertices (neighbourhoods), expanded
edges (edges admitted), graph storage reads (index entries + row fetches + full rows, split below), returned
paths and peak accounted query memory after the cold and warm runs. The generic memory-mode `storage_reads`
counter is 0 in every case and is not evidence of the bound.

### Cold and warm counters

Cold and warm are identical in every case: in memory mode with result caching off there is no
cross-statement cache, so the cold run pays no extra storage reads. The cold/warm storage-read difference is
therefore 0 here; a disk-backed cold run (OS and block cache) is not measured and stays open.

| Owner / case | Paths | Validated | Exp. vertices | Exp. edges | Index entries | Row fetches | Full rows | Peak bytes (cold / warm) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `graph_degree_scaling_query / degree-16` | 16 | 0 | 1 | 16 | 33 | 33 | 0 | 129,588 / 129,588 |
| `graph_degree_scaling_query / degree-64` | 64 | 0 | 1 | 64 | 129 | 129 | 0 | 493,716 / 493,716 |
| `graph_degree_scaling_query / degree-256` | 256 | 0 | 1 | 256 | 513 | 513 | 0 | 1,951,016 / 1,951,016 |
| `graph_depth_scaling_query / depth-2` | 1 | 0 | 2 | 2 | 5 | 5 | 0 | 24,233 / 24,233 |
| `graph_depth_scaling_query / depth-4` | 1 | 0 | 4 | 4 | 9 | 9 | 0 | 37,937 / 37,937 |
| `graph_depth_scaling_query / depth-8` | 1 | 0 | 8 | 8 | 17 | 17 | 0 | 66,149 / 66,149 |
| `graph_fanout_scaling_query / fanout-2` | 8 | 0 | 7 | 14 | 29 | 29 | 0 | 107,304 / 107,304 |
| `graph_fanout_scaling_query / fanout-4` | 64 | 0 | 21 | 84 | 169 | 169 | 0 | 617,816 / 617,816 |
| `graph_fanout_scaling_query / fanout-8` | 512 | 0 | 73 | 584 | 1,169 | 1,169 | 0 | 4,407,556 / 4,407,556 |
| `graph_skewed_hub / hub-anchored` | 2000 | 0 | 1 | 2,000 | 4,001 | 4,001 | 0 | 15,136,421 / 15,136,421 |
| `graph_skewed_hub / ordinary-avoids-hub` | 4 | 0 | 3 | 6 | 13 | 13 | 0 | 51,172 / 51,172 |
| `graph_skewed_hub / ordinary-through-hub` | 2004 | 0 | 4 | 2,007 | 4,012 | 4,012 | 0 | 15,170,190 / 15,170,190 |
| `graph_disconnected_growth / 1x` | 64 | 0 | 21 | 84 | 169 | 169 | 0 | 618,020 / 618,020 |
| `graph_disconnected_growth / 10x` | 64 | 0 | 21 | 84 | 169 | 169 | 0 | 618,020 / 618,020 |
| `graph_authorization_reachability_query / 100k` | 3 | 0 | 3 | 4 | 8 | 8 | 0 | 39,315 / 39,315 |
| `graph_hierarchy_ancestry_query / 100k` | 15 | 0 | 16 | 15 | 31 | 31 | 0 | 139,827 / 139,827 |
| `graph_friends_of_friends_query / 100k` | 9 | 0 | 2 | 6 | 9 | 9 | 0 | 61,130 / 61,130 |
| `graph_weighted_routes_query / 100k` | 3 | 70,000 | 4 | 10 | 15 | 15 | 0 | 134,093 / 134,093 |

Warm counters equal cold counters for every row above; the JSON records them separately.

### Plan, fallback and oracle per case

Every case below was answered by the lazy indexed expansion (`answered_by = indexed`): endpoint-index edge
access in `EXPLAIN`, zero full rows, no fallback label, zero result-cache hits. A case answered by a cached
result or a full-scan fallback is reported as such in its record and raises a finding that fails the bench
run; none occurred.

| Owner / case | Plan | Indexed vs oracle | Exact vs oracle |
| --- | --- | --- | --- |
| `graph_degree_scaling_query / degree-16` | indexed, no fallback | matched (16 rows) | matched |
| `graph_degree_scaling_query / degree-64` | indexed, no fallback | matched (64 rows) | matched |
| `graph_degree_scaling_query / degree-256` | indexed, no fallback | matched (256 rows) | matched |
| `graph_depth_scaling_query / depth-2` | indexed, no fallback | matched (1 rows) | matched |
| `graph_depth_scaling_query / depth-4` | indexed, no fallback | matched (1 rows) | matched |
| `graph_depth_scaling_query / depth-8` | indexed, no fallback | matched (1 rows) | matched |
| `graph_fanout_scaling_query / fanout-2` | indexed, no fallback | matched (8 rows) | matched |
| `graph_fanout_scaling_query / fanout-4` | indexed, no fallback | matched (64 rows) | matched |
| `graph_fanout_scaling_query / fanout-8` | indexed, no fallback | matched (512 rows) | matched |
| `graph_skewed_hub / hub-anchored` | indexed, no fallback | matched (2000 rows) | matched |
| `graph_skewed_hub / ordinary-avoids-hub` | indexed, no fallback | matched (4 rows) | matched |
| `graph_skewed_hub / ordinary-through-hub` | indexed, no fallback | matched (2004 rows) | matched |
| `graph_disconnected_growth / 1x` | indexed, no fallback | matched (64 rows) | matched |
| `graph_disconnected_growth / 10x` | indexed, no fallback | matched (64 rows) | matched |
| `graph_authorization_reachability_query / 100k` | indexed, no fallback | matched (3 rows) | unavailable, SQLSTATE 54000 (query budget) |
| `graph_hierarchy_ancestry_query / 100k` | indexed, no fallback | matched (15 rows) | unavailable, SQLSTATE 54000 (query budget) |
| `graph_friends_of_friends_query / 100k` | indexed, no fallback | matched (9 rows) | unavailable, SQLSTATE 54000 (query budget) |
| `graph_weighted_routes_query / 100k` | indexed, no fallback | matched (3 rows) | not applicable (oracle is the exact comparator) |

The exact full read of the three 100k `GRAPH_TABLE` representatives does not fit the 64 MiB budget (`54000`),
as `tier3_system_graph` already documents. That is a known limit of the exact path, not an indexed-path
finding; their indexed answers are compared with the oracle instead.

### Disconnected growth

The queried neighbourhood (a fanout-4, depth-3 tree rooted at vertex 1, 64 result rows) is fixed. Disconnected
50-vertex rings grow 10x: 1x is 20 rings (1,085 vertices, 1,084 edges),
10x is 200 rings (10,085 vertices, 10,084 edges).

| Growth | Expanded vertices | Expanded edges | Index entries | Row fetches | Full rows | Exact read full rows |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1x | 21 | 84 | 169 | 169 | 0 | 2,169 |
| 10x | 21 | 84 | 169 | 169 | 0 | 20,169 |

Indexed anchored expansion touched identical counters at 1x and 10x (cold and warm), so it touched no
additional disconnected edges. The exact read scales with the graph, which is the contrast the bench
records. The bench raises a finding if the indexed counters differ.

### Skewed hub

Seed `0x5EED1355`; vertex 1 is a hub with out-degree 2,000 among 4,000 ordinary vertices with two seeded
out-edges each; ordinary vertex 3,000 has one edge into the hub. The hub costs what its degree costs when
the query expands it and nothing otherwise.

| Case | Paths | Exp. vertices | Exp. edges | Index entries |
| --- | ---: | ---: | ---: | ---: |
| hub-anchored | 2000 | 1 | 2,000 | 4,001 |
| ordinary-avoids-hub | 4 | 3 | 6 | 13 |
| ordinary-through-hub | 2004 | 4 | 2,007 | 4,012 |

### 100k weighted representative

`graph_weighted_paths` over the unchanged 100,000-row fixture (30,000 people, 70,000 edges) returned its three
oracle paths (ranks and costs match the plain-Rust k-cheapest simple-path oracle) with peak accounted query
memory of 134,093 bytes against the 67,108,864 byte (64 MiB) budget.
Validation and expansion are separate. The validation scan read 70,000 edge rows (every
selected edge, by contract). The search expanded 4 vertices and 10 edges
with 15 index entries and 15 row fetches and no full-row read. Validation is
therefore the dominant bounded cost, about 7,000 times the expanded edges. It stays
O(selected edge rows) and no sidecar removes it.

### Reopened local weighted fixture

The opt-in run loads the same 30,000 people and 70,000 edges into a fresh local
directory, closes the engine and opens the committed fixture again. It executes
the weighted query twice on the same reopened instance, then captures the plan.
Both invocations must match the independent three-path oracle and validate all
70,000 edges. Each invocation must also report indexed expansion, zero full
expansion rows and zero result-cache hits. The runtime query budget remains
64 MiB. The artifact records the native limits read back from the local engine,
which uses the local constructor defaults.

```sh
CASSIE_BENCH_GRAPH_DISK_REOPEN=1 \
  cargo bench --locked -p cassie-e2e --bench tier3_system_graph
```

The report is `target/graph-evidence/tier3_system_graph_disk_reopen.json` unless
`CASSIE_BENCH_GRAPH_EVIDENCE_DIR` is set. Cold means the first graph query on
the reopened engine. The operating system file cache can retain pages from
setup. The storage counters describe logical work and this run records no
latencies. The default invocation still runs the memory workloads.

For a weighted timing comparison, run the same memory workload separately from
the issue baseline `33a80d480c1436f9beb2b422a6fd6b29d4fadf` and the candidate.
From each checkout's repository root, use the same sample selection and profile
with the disk option unset. Each checkout retains its own results.

```sh
cargo bench --locked --bench tier3_system_graph -- \
  --workload graph_weighted_routes_query --samples 30 --warmup-samples 5
```

Repeat on the same host with matched controls. Retain each artifact and its
source, host, configuration and measured noise before interpreting the p95
difference. This procedure does not supply the missing comparison.

### What stays open

- Repeated timing distributions, the p95 comparison against the paired baseline and the first
  degree/depth/work limit are not produced: no quiet machine is available. The previously noisy
  `graph_hierarchy_ancestry_query`, `graph_friends_of_friends_query` and `graph_weighted_routes_query`
  rows remain **pending**, and the p95 values in the earlier table are diagnostics only.
- Disk-backed cold versus warm storage reads are not measured.
- These counters come from one smoke run. They are deterministic but were not repeated.

## Acceptance map and remaining qualification work

| Requirement | Evidence |
| --- | --- |
| Type/identity/encoding/JSON contract | [Graph contract](graph-v060-contract.md), [type owners](../tests/graph_types.rs) |
| Typed keys, lifecycle, rename/restart, replacement and clone isolation | Graph-type suite 106 passed; mapped-graph suite 149 passed in the complete run |
| Real clients, native paths/arrays, text/binary, escaping and precision | `tokio-postgres` owners plus the executed Psycopg workflow above; scalar SQLx owners remain separate |
| Query semantics, transaction view and independent exact oracles | Graph-table 149 and weighted-path 116 tests passed in the complete run |
| Resource controls and no successful partial result | Graph-table/weighted/type memory and cancellation owners in those passed suites |
| Native/fallback EXPLAIN and measured read bounds | [Fast-read owners](../tests/graph_table/fast_reads.rs) passed, including fallback ANALYZE counters |
| Benchmarks registered across kernels, algorithms, representatives and topology axes | 20 executed scenario rows; registry and source-contract diagnostics passed |
| Read-model user guide | [Graph guide](graph-guide.md); all 11 guide tests passed |
| Qualification boundary | Local correctness is separate from failed/noisy timing quality |

Future graph performance qualification must resolve the failed timing-quality
evidence against a named deployment-profile baseline. Production latency and
capacity claims separately need representative operational evidence. Native
Linux disk-backed measurements and a complete release-suite artifact are not
supplied here. SQLx native composite auto-discovery, identity-preserving
image restore/remapping, and full SQL/PGQ conformance remain outside the selected
contract. The guide and performance contract state the supported indexed-read
bounds, weighted validation cost and exact fallback behavior.
