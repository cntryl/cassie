# Filtered vector search contract

This contract belongs to [#1363](https://github.com/cntryl/cassie/issues/1363) and extends the
[vector metric qualification](vector-metric-qualification.md) and the
[performance contracts](performance-contracts.md). It adds no type, public API, persistent
format or storage layer. Midge remains the only storage layer and the strategy reads only
existing scalar-index, vector-index and posting state through Cassie adapters.

## Eligible filter shape

A statement is eligible when it is a native vector top-k (`ORDER BY embedding <-> q`,
`<=>`, `<#>`, or `vector_distance(embedding, q)`, with `LIMIT` and optional `OFFSET`) whose
`WHERE` clause is a conjunction containing at least one `column = literal` or `column = $n`
conjunct over a column with an eligible scalar index. A scalar index is eligible when it is a
non-partial scalar index whose first key is the column, its probe comparison is exact for the
column type, and it is visible to the session. Every other conjunct is a residual predicate. The
residual must be supported by the vector prefilter evaluator (`vector_prefilter_supported`).

The eligible conjuncts are probed with the same exactness guard used by indexed full-text reads
(`scalar_equality_candidates`) and intersected into one id subset `S`. Every fetched row is
rechecked against the whole `WHERE` clause, so the residual and the index probes cannot disagree
about membership.

Hybrid statements (`hybrid_score(search_score(f, q), vector_score(v, qv))`) use the same subset
from the equality conjuncts of `WHERE search(f, q) AND tenant = $n [AND ...]`. The `search(f, q)`
conjunct that matches the scored text field and query is implied by hybrid scoring and is
removed from the residual.

## Selection rule

With `needed = OFFSET + LIMIT` and `adaptive_max = CASSIE_ADAPTIVE_CANDIDATE_MAX` (default
100,000):

1. **Bounded exact on the scalar subset** when `|S| <= min(max(32 * needed, 256), adaptive_max)`.
   Every row of `S` is fetched, rechecked and exactly reranked. The result is exact. Also used
   when no usable ANN index exists (no index, descending order, mismatched metric, zero query,
   missing manifest) and `|S| <= adaptive_max`.
2. **Filtered ANN** otherwise. Candidate windows grow until the eligible-candidate target
   `min(|S|, 4 * needed)` is met, then the eligible candidates are exactly reranked from source
   rows. The result is an approximate profile; see the recall floor below.
   - HNSW: the first window is `max(16 * needed, 64)` candidates and doubles up to
     `adaptive_max`. Candidates are ordered by distance and the first eligible ones are kept.
     Only a window that returned every indexed row proves exhaustion. A search that stops
     returning new rows is limited by graph reach and declines (`ann-graph-reach-limited`).
   - IVFFlat: probing starts at the manifest's `probes` and doubles up to the list count, all
     lists being probed last. Eligible members of the probed lists are reranked.
3. **Full exact fallback** with a labelled reason when no eligible scalar conjunct exists, the
   filter is unsupported, or `|S| > adaptive_max` with no ANN index.

A filtered ANN attempt that cannot fill its window never returns an underfilled success. It
degrades to rule 1 (when `|S| <= adaptive_max`) or rule 3. After reranking, the result must hold
at least `min(needed, |S|)` rows or the attempt is discarded (`ann-window-underfilled`).

## Fallback reasons

Recorded in `vector_filtered.fallback_reasons` and, for the attempted ANN index, the existing
`vector.*_fallbacks` counters:

| Reason | Meaning |
|---|---|
| `no-eligible-scalar-index` | No equality conjunct is served by an eligible scalar index. Full exact scan. |
| `unsupported-filter` | The filter contains an expression the vector prefilter evaluator rejects. |
| `subset-over-candidate-cap` | `\|S\| > adaptive_max` and no ANN index can serve the query. |
| `ann-window-underfilled` | Reranked eligible candidates were fewer than `min(needed, \|S\|)`. Degrades to bounded exact. |
| `ann-graph-reach-limited` | The HNSW search stopped returning new rows before enough eligible rows were found. |
| `ann-candidate-cap` | The window reached `adaptive_max` without enough eligible rows. |
| `ann-<index reason>` | The index declined (`missing-graph`, `stale-graph`, `missing-training`, ...). |
| `concurrent-source-change` | The collection generation moved, or a candidate row vanished. |
| `transaction-overlay` / `transaction-overlay-exact` | The session has uncommitted changes to the collection. Hybrid and vector use their existing exact paths. |
| `missing-text-index`, `invalid-text-artifact`, `missing-candidate-row` | Hybrid only: the existing bounded or exact path runs. |

## Hybrid composition

`hybrid_score` exists only for rows with a lexical match, and its vector component is the exact
L2 distance of the fetched row. The exact candidate set of `search(..) AND tenant = $n` is
therefore the persisted postings restricted to `S`: a vector candidate window can only lose rows,
never add one. The filtered hybrid path intersects the postings with `S`, fetches the surviving
rows only, rechecks the residual, and scores them with the corpus-wide persisted BM25 statistics
(`0.65 * bm25 + 0.35 / (1 + L2)`). Its result equals the exact row scan. It reads no ANN window
(`hybrid.ann_reads_total` stays zero) with or without a vector index. `|S|` and the posting
candidate count are capped by `adaptive_max`; beyond it the existing paths run.

## Result and safety invariants

* Distances are the exact distances of source rows under the operator metric; ties order by
  distance then row identity, as in every other vector path.
* The collection generation is captured before the scalar probe and fenced before every row
  fetch, after reranking and at the end. A change declines to the exact path.
* Transaction overlays never use committed indexes. They run the labelled exact path.
* Memory is reserved for the subset ids, window ids, candidates and fetched rows against the
  query budget. Cancellation and deadlines are checked per candidate. A budget failure is a
  `ResourceLimit` error; it is never turned into a partial result.

## Recall floor and exactness labels

* Bounded exact (standalone and hybrid) is **exact**.
* Filtered ANN is an **approximate profile** with the repository's `0.90` recall@k floor
  (`docs/performance-contracts.md`), measured against an independent filtered oracle.
* The strategy only trusts ANN candidates after exact reranking; it cannot return a wrong
  distance, only a missed neighbor.

## EXPLAIN

For a filtered native vector top-k over a collection with an ANN index, EXPLAIN reports the
strategy it would select, using the same scalar probe and selection rule as execution:

```
vector_ann=filtered-ann vector_ann_reason=adaptive-window vector_filter_subset=400 ...
vector_ann=bounded-exact-subset vector_ann_reason=subset-within-exact-limit vector_filter_subset=40 ...
vector_ann=structured-filter-exact vector_ann_reason=no-eligible-scalar-index vector_filter_subset=none ...
```

## Evidence

`tests/vector_embeddings/filtered/` (standalone) and
`tests/vector_embeddings/hybrid/tenant_hybrid_candidates.rs` (hybrid) cover selectivity,
fill, residuals, EXPLAIN labels, update/delete, rollback, transaction isolation and lowered
budgets. Fixtures live in `tests/support/vector_filtered.rs` and reuse
`tests/support/ranked_search.rs`. Pages are checked against independent oracles.

Fixture note: HNSW graphs built from vectors inserted in distance order, or over a strong
density gradient, can leave regions unreachable. The fixtures therefore insert in an order
unrelated to distance and use constant density; the strategy additionally detects and labels
the reach-limited case instead of returning an underfilled page.
