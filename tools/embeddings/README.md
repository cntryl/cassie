# Embedding workload generation (#1365)

Workstation-only machinery that produces real semantic embeddings for the opt-in
`tier5_embedding_workloads` benchmark. Nothing here runs in CI or `cargo test`. No corpus
text and no vectors are committed or redistributed: the committed
[`tests/corpus/embeddings/manifest.json`](../../tests/corpus/embeddings/manifest.json) holds
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
