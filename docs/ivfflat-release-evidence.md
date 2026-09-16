# IVFFlat Release Evidence

IVFFlat is a Stable functional capability. This record defines representative native-disk
release evidence without turning the Stable label into a global capacity claim.

## Functional and Recovery Contract

The `ivfflat_indexes`, `ivfflat_completeness`, `vector_query_stability`, and
`vector_ann_concurrency` modules in `tests/vector_embeddings.rs` and
`tests/query_scan_controls.rs` cover option persistence, trained membership-prefix reads,
refresh after writes, exact source-row reranking, deterministic recall, restart hydration and
rebuild, deletion visibility, generation fencing, cancellation, memory exhaustion, stale or
incomplete state, invalid list bounds, unsupported training versions, and publication metrics.
`should_clean_batched_ivfflat_sidecars_after_failed_publication_retry` injects failure before
manifest publication, proves the partial training remains unpublished, retries successfully, and
proves deletion removes normalized-vector and membership sidecars.

## Representative Matrix

All representative fixtures use the deterministic modular three-dimensional `bench_documents`
fixture with fixture seed `0`, L2 distance, top-k `20`, training seed `42`, and the immutable
recall floor `0.90`.

| Owner | Scale | Lists | Probes | Training sample | Filter profile |
| --- | ---: | ---: | ---: | ---: | --- |
| `tier2_subsystem_vector` | 1,024 | 16 | 4 | 1,024 | Kernel probe selection |
| `tier5_scaling_retrieval` | 10,000 | 16 | 4 | 1,024 | Unfiltered ANN |
| `tier3_system_query` | 100,000 | 64 | 16 | 4,096 | Unfiltered ANN |
| `tier5_scaling_retrieval` | 100,000 | 16 | 4 | 1,024 | Unfiltered ANN |
| `tier5_scaling_retrieval` | 250,000 | 16 | 4 | 1,024 | Unfiltered ANN |

The retained ANN rows use the unfiltered profile so candidate and membership-read scaling remains
comparable. Structured-filter correctness and its explicitly diagnosed exact fallback are
functional gates, not hidden inside the latency measurement.

Fixture construction, exact-top-k baseline calculation, index training, recall calculation, and
EXPLAIN preflight remain outside the timed region. Each retained representative row records
recall, floor, top-k, dimensions, metric, fixture and training seeds, list/probe/training sizes,
filter profile, selected path, fallback diagnostics, latency distribution, candidates,
membership-prefix and point reads, peak accounted memory, workers, fixture identity, commit,
toolchain, and deployment profile. Artifact validation rejects missing fields, a weakened floor,
sub-floor recall, or configuration drift.

## Retained Evidence

The exact-head native-Linux disk run and artifact links are recorded in
[Production Readiness](production-readiness.md) after the release-candidate commit completes. The
focused smoke command validates shape only:

```sh
STRESS_PROFILE=smoke \
STRESS_FILTER=perf.vector.ivfflat_persisted.10k \
cargo bench --locked --bench tier5_scaling_retrieval
```

HNSW remains separately owned by issue #24. Hosted embedding-provider availability remains
operator-owned and is not part of IVFFlat release evidence.
