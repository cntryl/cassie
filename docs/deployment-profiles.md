# Deployment Profiles

These named profiles define how Cassie benchmark and readiness evidence is collected. A
profile is a reproducible comparison boundary, not an SLA. Local fallback-storage runs are
diagnostic and must not be recorded as disk-backed deployment evidence.

## `workstation-apple-m5-arm64-apfs`

- Host: Apple M5, arm64, macOS, APFS.
- Storage: disk-backed `CASSIE_MIDGE_DATA_DIR` on the local APFS volume.
- Runtime: repository defaults unless a benchmark owner declares an override in the artifact.
- Fixtures: Tier 1-4 declared workload fixtures plus Tier 5 scale axes and both Tier 6 endurance scenarios.
- Workload: mixed SQL reads/writes, retrieval, projections, transport, restart, and recovery-sensitive paths.
- Toolchain: exact `rust-toolchain`/Cargo lock state recorded with the artifact.
- Status: existing local evidence profile; not representative native-Linux production evidence.

## `native-linux-amd64-disk`

- Host: Linux amd64, with the CPU, memory, kernel, filesystem, and mounted-volume identity recorded.
- Storage: disk-backed `CASSIE_MIDGE_DATA_DIR` on the declared native filesystem; fallback storage is not valid.
- Runtime: repository defaults plus explicitly recorded profile overrides for admission, memory, workers, and timeouts.
- Fixtures: the same Tier 1-6 owner and scale contract as the Apple profile wherever the owner applies.
- Workload: the same named mixed workload and client/worker axes, preserving fixture and query-shape identity.
- Toolchain: exact Rust toolchain, lockfile, Cassie commit, Midge revision, and benchmark run ID recorded.
- Status: required representative-Linux profile; evidence remains pending until a complete retained run exists.

## Diagnostic smoke profiles

`local-dev-fallback-10k` and `local-dev-fallback-100k` may be used for fast feedback. They use
the configured fallback only when tests explicitly opt in, and their artifacts must be marked
`diagnostic` rather than `complete`. They cannot replace either disk-backed profile or establish
production thresholds.

## Artifact and ownership contract

Each retained artifact is named
`<profile>/<commit>/<run-id>/<owner>/<scenario>.json` and records the profile, host and
filesystem, Cassie/Midge revisions, toolchain, command, fixture identity, runtime overrides,
result rows, p50/p95/p99, throughput, fallback reasons, storage reads/writes, candidates, peak
accounted memory, workers, and cancellation latency where applicable.

The benchmark owner validates schema and completeness; the release-readiness owner compares
only artifacts with matching profile, fixture, toolchain, and commit contracts. Retain the
latest complete manifest plus the prior complete manifest for comparison, and retain diagnostic
artifacts separately. Any missing axis, filtered owner, mixed commit, or smoke artifact keeps the
profile evidence incomplete.
