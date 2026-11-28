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

The engine's `perf.sql.parameter_type_admission.128` scenario measures type
admission during binding, while the historical `perf.sql.parameter_binding.128`
measured wire value decoding, now covered by `bind_parameter_decode` in the
server's `tier2_subsystem_protocol_handlers` benchmark.

GitHub Actions uses `ubuntu-24.04-arm` and the repository-pinned Rust 1.100.0.
Benchmark smoke compiles workspace bench targets on relevant pull requests;
manual dispatch also runs registered timed and product smoke owners.

| Workflow | Profiles | Trigger |
| --- | --- | --- |
| **Benchmark Tiers 1–4** (`benchmark-tiers.yml`) | `smoke`, `standard`, `full` | Manual or daily |
| **Benchmark Tier 5 Saturation** (`saturation.yml`) | `smoke`, `standard`, `full` | Manual |
| **Benchmark Tier 6 Endurance** (`endurance.yml`) | `smoke`, `full` | Manual |

Tiers 1–4 can run together or individually. Each timed owner gets its own job
and report, so a failed owner leaves other reports available. The scheduled
Tiers 1–4 run uses `full`. Tier 6 full runs require at least 3,600 seconds of
measurement and have a six-hour job limit. Smoke results are diagnostic.

The registry classifies `relevance_cranfield`, `tier3_replay_durability`, and
`tier4_pgwire_fixture_profile` as diagnostics. The smoke workflow compiles
those targets and the measurement workflows leave them for direct Cargo runs.

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
