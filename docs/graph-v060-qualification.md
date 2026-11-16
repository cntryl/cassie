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
