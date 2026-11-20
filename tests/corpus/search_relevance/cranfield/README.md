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

Cassie must match the independent BM25 reference on every query: identical top-10 ids in identical
order and scores within the manifest tolerance. nDCG@10 and MRR@10 are reported for both and are
not thresholded. See `manifest.json` for the parsing, query-numbering and relevance-level
conventions and the level-to-grade mapping.

The network-free checks (manifest, parsers, mapping, reference) are in
`tests/search/retrieval/cranfield_workload.rs` and run with the `search` test binary.
