# HNSW Release Evidence

HNSW is a Stable functional capability. This record defines the narrower release-evidence
contract used to validate its representative native-disk behavior; it does not make Cassie
globally Production-ready or guarantee capacity on an operator's hardware.

## Functional and Recovery Contract

The `hnsw_indexes`, `vector_query_stability`, and `vector_ann_concurrency` modules in
`tests/vector_embeddings.rs` and `tests/query_scan_controls.rs` cover option persistence,
restart hydration, exact source-row reranking, deletion visibility, generation fencing,
cancellation, memory exhaustion, provider/model/dimension mismatches, and explicit fallback for
missing, stale, or corrupt graphs. `should_clean_batched_hnsw_sidecars_after_failed_publication_retry`
injects failure before manifest publication, proves the incomplete graph is unpublished, retries
the build, and proves index deletion removes both normalized-vector and graph-node sidecars.

## Representative Matrix

The release matrix uses the deterministic modular three-dimensional `bench_documents` fixture
with fixture seed `0`, L2 distance, top-k `20`, `m=32`, `ef_construction=256`, and
`ef_search=256`.

| Owner | Scale | Purpose |
| --- | ---: | --- |
| `tier5_scaling_retrieval` | 10,000 | Small representative disk fixture |
| `tier3_system_query` | 100,000 | Representative system-query fixture |
| `tier5_scaling_retrieval` | 100,000 | Scaling-curve cross-check |
| `tier5_scaling_retrieval` | 250,000 | Declared upper scaling fixture |

Fixture construction, exact-top-k baseline calculation, index construction, recall calculation,
and EXPLAIN preflight are outside the timed region. Every retained HNSW summary records
`recall_at_k`, the immutable `0.90` recall floor, top-k, dimensions, metric, fixture seed, graph
parameters, selected access path, fallback diagnostics, latency distribution, candidate count,
storage reads, peak accounted memory, configured workers, leaked workers, fixture identity,
commit, toolchain, and deployment profile. Artifact validation rejects missing configuration,
a weakened floor, or observed recall below the floor.

## Retained Evidence

The exact-head native-Linux disk run and artifact links are recorded in
[Production Readiness](production-readiness.md) after the release-candidate commit completes.
The focused smoke command below validates shape only and cannot satisfy retained release evidence:

```sh
STRESS_PROFILE=smoke \
STRESS_FILTER=perf.vector.hnsw_persisted.10k \
cargo bench --locked --bench tier5_scaling_retrieval
```

IVFFlat has a separate evidence owner in issue #25 and is intentionally not promoted by this
record. Hosted embedding-provider availability remains operator-owned and separate from the HNSW
index contract.
