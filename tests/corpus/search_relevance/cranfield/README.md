# Cranfield relevance evaluation (#1368)

An independent, human-judged workload for Cassie full-text ranking: the Cranfield collection
(1,400 aerodynamics abstracts, 225 queries, graded relevance judgments).

The collection has no published license, so it is **not stored in this repository**. `manifest.json`
pins its source URL, SHA-256 and size; the runner downloads it into `target/relevance/cranfield/`
and refuses any archive that does not match.

## Run

```sh
cargo bench --locked --bench relevance_cranfield
```

This needs the network on the first run (a verified cached archive is reused afterwards) and the
system `tar`. It is not part of `cargo test`. Results are written to
`target/relevance/cranfield/results.json` and a summary is printed. The process exits non-zero when
Cassie disagrees with the independent BM25 reference.

## Contract

The runner evaluates each analyzer configuration listed in `manifest.json`
(`default`, and `english_stemming` with `WITH (stemming = english)`).

1. **Correctness (hard gate):** for every configuration, Cassie matches the independent BM25
   reference on every query: identical top-10 ids in identical order and scores within the
   manifest tolerance.
2. **No regression:** each configuration's mean nDCG@10 and MRR@10 stay at or above its pinned
   floor, set 0.005 below the first measured value.
3. **Quality:** `english_stemming` scores at least `default` on both means.

No absolute 0.80 floor applies to Cranfield: exact BM25 scores far below it on this collection.
First measured on 2026-11-20 (225 queries, cutoff 10):

| Configuration | nDCG@10 | MRR@10 | Pinned floors |
|---|---|---|---|
| `default` | 0.3311 | 0.4609 | 0.326 / 0.456 |
| `english_stemming` | 0.3551 | 0.4805 | 0.350 / 0.475 |

See `manifest.json` for the parsing, query-numbering and relevance-level conventions and the
level-to-grade mapping.

The network-free checks (manifest, parsers, mapping, reference) are in
`tests/search/retrieval/cranfield_workload.rs` and run with the `search` test binary.
