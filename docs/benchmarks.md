# Benchmark navigation

The [performance contracts](performance-contracts.md) own benchmark scope and
acceptance rules. The [local replay driver](../benches/tier3_replay_durability.md)
records write/clean-reopen diagnostics; the separately selected
[projection replay profiles](projection-replay-profiles.md) define the finite
read campaign, source-copy ownership, execution-path attestation and operator
interpretation. Diagnostic profiles and counters are not durability receipts.

## Running registered benchmarks

`tools/benchmark_workflows.py` reads the shared `cassie-bench` registry and
runs one owner with the same command locally and in Actions. Use a Linux arm64
host for comparable native-disk evidence. The command preserves local Cargo
and benchmark environment overrides.

```sh
# List registered timed owners in a tier.
python3 tools/benchmark_workflows.py list --tiers 3

# Run an owner directly with Cargo.
cargo bench --locked -p cassie-sql --bench tier2_subsystem_parser

# Run one owner in its release profile, writing cntryl-stress output below target/stress.
python3 tools/benchmark_workflows.py run tier3_system_query --profile full

# Run a deterministic, diagnostic smoke profile.
python3 tools/benchmark_workflows.py run tier3_system_query --profile smoke
```

GitHub Actions uses `ubuntu-24.04-arm` and the repository-pinned Rust 1.100.0.
Benchmark smoke compiles workspace bench targets and runs registered timed and
product smoke owners on relevant pull requests. Nightly runs Tiers 1–4
serially. Manual tier dispatch selects one tier or all six; Tier 5 runs one
owner per job, and every Tier 6 soak has a separate job with a six-hour job
limit and a minimum 3,600-second full measurement. Raw owner artifacts upload
even when an owner fails. Smoke artifacts are diagnostic and do not establish
performance targets.

The product workflow is manually dispatched from `main` on the same Linux
arm64 runner. It runs non-vector owners serially. A vector run selects one of
`exact`, `hnsw-1`, `hnsw-2`, `ivfflat-1`, or `ivfflat-2` and one half-open
query-ID range such as `0..250`; repeat the dispatch for other ranges and
configurations. Full vector runs require the private
`CASSIE_BENCH_VECTOR_INPUT_URL` HTTPS archive and verify pinned input hashes in
the benchmark owner. Corpus inputs and the downloaded archive are never
uploaded as workflow artifacts; only product results are retained. Missing
private input blocks hosted full vector measurement without affecting other
product or timed runs.
