# Embedding workload generation (#1365)

Workstation-only machinery that produces real semantic embeddings for the opt-in
`tier5_embedding_workloads` benchmark. Nothing here runs in CI or `cargo test`. No corpus
text and no vectors are committed or redistributed: the committed
[`crates/cassie-e2e/tests/corpus/embeddings/manifest.json`](../../crates/cassie-e2e/tests/corpus/embeddings/manifest.json) holds
the recipe, pins, query splits and checksums, and every contributor regenerates the files
under `target/embeddings/` (git-ignored).

## Sources

- Corpus: CQADupStack (StackExchange duplicate-question retrieval) in its BEIR packaging,
  Hugging Face dataset `BeIR/cqadupstack` at the revision pinned in the manifest (card
  license `cc-by-sa-4.0`; upstream repository <https://github.com/D1Doris/CQADupStack>,
  Apache-2.0 code, cite Hoogeveen et al., ADCS 2015). All 12 subforums are combined.
- Models: `BAAI/bge-small-en-v1.5` (384 dimensions) and `BAAI/bge-base-en-v1.5` (768
  dimensions), both MIT, at the commit revisions pinned in the manifest.

## Pipeline

1. Documents are every BEIR corpus document that is not a BEIR query document, ordered by
   `sha256("select:" + id)` (then id); a set is the first N, so 10k is a prefix of 100k.
2. Text is `title + "\n" + body`, whitespace-normalized, truncated by the model to 256
   tokens. Queries are the BEIR query documents embedded identically.
3. Vectors are L2-normalized and stored as little-endian float32 rows. Chunks of 1000
   documents are embedded independently so the 10k files are a bit-exact prefix of the 100k
   files on one machine and library set.
4. Queries are ordered by `sha256("split:" + id)`; the first 1000 are the tuning ids, the
   next 1000 are the held-out ids. They are frozen in the manifest.
5. Tenant columns `t1`, `t10`, `t50` (1%, 10%, 50% selectivity) come from
   `sha256("tenant:" + id) mod 100` and are computed by the Rust runner from the ids file.

## Commands

```sh
# once, from the repository root (Python 3.12)
python3.12 -m venv target/embeddings/venv
target/embeddings/venv/bin/pip install -r tools/embeddings/requirements.txt

# 10k set for both models (fills file checksums and query splits into the manifest)
target/embeddings/venv/bin/python tools/embeddings/generate.py \
  --models bge-small,bge-base --sets 10k --update-manifest

# 10k and 100k sets for both models (100k contains the 10k prefix)
target/embeddings/venv/bin/python tools/embeddings/generate.py \
  --models bge-small,bge-base --sets 10k,100k --update-manifest
```

Pass `--threads N` (and `OMP_NUM_THREADS=N`) to cap the torch CPU threads on a shared
machine. Thread count can change the last bits of the vectors, so a regenerated set records
its own checksums.

Expected cost on a 10-core Apple-silicon laptop CPU (device `cpu`): about 3.5 minutes for
`bge-small` and 5 to 6 minutes for `bge-base` per 10k documents, so roughly 35 and 60
minutes for the 100k set. Downloads are about 1 GB of parquet plus two model checkpoints
(under `target/embeddings/cache/`). Output: 15 MB (10k, 384d), 31 MB (10k, 768d), 154 MB
(100k, 384d), 307 MB (100k, 768d), plus small id files.

Generation prints the SHA-256 of every output. `--update-manifest` writes them into the
manifest; commit the manifest change. A different CPU or library version can change the
last bits, so the checksums identify the exact run; regenerate and re-record them if the
loader reports a mismatch.

## Running the benchmark

```sh
CASSIE_BENCH_EMBEDDING_WORKLOADS=1 CASSIE_BENCH_EMBEDDING_SETS=10k \
  cargo bench --locked --bench tier5_embedding_workloads
```

Optional: `CASSIE_BENCH_EMBEDDING_MODELS` (default `bge-small,bge-base`),
`CASSIE_BENCH_EMBEDDING_TUNING_POINTS` (default 2 per index, maximum 2),
`CASSIE_BENCH_EMBEDDING_QUERY_LIMIT` (caps queries per split, smoke runs only).

The runner verifies the SHA-256 of every file against the manifest and stops with a pointer
here when a file is missing or changed. It computes exact top-20 ground truth by brute force
(ordering and tie policy in the manifest), caches it next to the vectors with a checksum
sidecar, loads the rows into a local-disk Cassie, and measures per metric (cosine, L2) x
algorithm (exact, HNSW, IVFFlat) x filter (unfiltered, 1%, 10%, 50%) x tuning point. Raw
results, including per-query recall and latency, are written to
`target/embeddings/results/*.json`. ANN rows and exact or exact-fallback rows are reported
separately and exact fallbacks never count as ANN evidence. Peak query memory is the
process peak of one configuration (index plus metric), shared by its filter rows.

## Synthetic control and tuning points

- `synthetic` is the deterministic control profile: 64 unit cluster centers plus per-component
  Gaussian noise, 384 dimensions, L2-normalized, generated in memory from the manifest
  `synthetic_control` section (SplitMix64 streams, `f32` arithmetic only, bit-identical on every
  platform). It has no files; the manifest records the SHA-256 of each generated set and the
  runner stops if a regenerated set differs. It uses the same 10k and 100k set sizes, the same
  tenant columns and the same tuning and held-out query labels as the real corpus.
- `index_tuning` in the manifest declares the swept candidates and the two selected points per
  index (HNSW `ef_search`, IVFFlat `probes` as permille of `lists`). Selection rule: use only the
  tuning split of the 10k set. Run `CASSIE_BENCH_EMBEDDING_SWEEP=1` (tuning split, every
  candidate), then per index take the smallest candidate whose mean recall@20 is at least 0.90
  in every ANN-served metric x filter cell of both real-embedding models, plus the next larger
  candidate. The synthetic control is swept and recorded (`index_tuning.selection_record`) but does
  not gate; it is the hard-negative control (its HNSW recall stays below 0.90 at every swept
  `ef_search`). The committed sweep used the first 250 tuning queries
  (`CASSIE_BENCH_EMBEDDING_QUERY_LIMIT=250`) on a shared machine.
- For k = 20 HNSW searches `max(ef_search, 64 * k) = max(ef_search, 1280)` candidates, so an
  `ef_search` below 1280 is the same operating point as 1280 and the grid starts there. Held-out
  queries are reported at the chosen points and never used to choose them.
- `CASSIE_BENCH_EMBEDDING_RECALL_ONLY=1` skips the exact configuration and the warmup; use it
  for recall and candidate counts at 100k. Its wall times are not quiet-machine measurements.
- `CASSIE_BENCH_EMBEDDING_ALGORITHMS` (`exact,hnsw,ivfflat`) and `CASSIE_BENCH_EMBEDDING_METRICS`
  (`l2,cosine`) split a run across processes; give each process its own
  `CASSIE_BENCH_EMBEDDINGS_DIR` (symlinks to the generated files are enough) because the work
  directory is shared by name.
- `CASSIE_BENCH_EMBEDDINGS_DIR` points the runner at another directory of generated files.
