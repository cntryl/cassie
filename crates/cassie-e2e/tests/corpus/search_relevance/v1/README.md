# Search relevance workload v1 (issue #1368)

A small, redistributable workload for judging whether Cassie's BM25 ranking puts a useful result
near the top. All text is original and written for this repository (support tickets, orders and a
product catalogue for a fictional office-supply retailer).

| File | Purpose |
| --- | --- |
| `corpus.json` | 43 documents. Each `id` is stored as the Cassie `_id`. |
| `intents.json` | 24 frozen search intents: 10 `development`, 10 `held_out` answerable, 4 `no_hit`. Kinds: exact identifier, description, repeated terms, ambiguous, no hit. |
| `judgments.json` | Independent human relevance grades. **Currently `"status": "unjudged"` and empty.** |

`authored_intended_doc_ids` and `authored_decoy_doc_ids` in `intents.json` record what the
fixture author designed each intent to find. They are not judgments and are never used to compute
nDCG or MRR. `held-04` ("ceramic floor tile") deliberately includes a keyword-stuffed decoy
(`SKU-711`) that out-ranks the genuine listing (`SKU-703`) under BM25, so an exact but irrelevant
ranking can fail the thresholds once a human grades the decoy as irrelevant.

The split is frozen: do not move intents between splits or change `query` text after judging
starts. Add a new workload version instead.

## How a human fills in `judgments.json`

1. Each grader works independently, without seeing Cassie's ranking or other graders' grades. They
   read `intents.json` (query only) and `corpus.json`.
2. For every intent in `development` and `held_out`, grade every document that could plausibly
   answer it, and any decoy, on this scale. Ungraded documents count as `0`.
   * `3` perfect: the document is exactly what the searcher wanted
   * `2` relevant: answers the need
   * `1` marginal: related but not sufficient
   * `0` irrelevant
3. Disagreements are resolved by the graders and the agreed grade recorded; keep the raw
   per-grader sheets next to the published results.
4. Write the file as:

```json
{
  "version": 1,
  "corpus_version": 1,
  "status": "judged",
  "graders": ["grader-a", "grader-b"],
  "grades": {
    "held-01": {"ORD-48207": 3},
    "held-04": {"SKU-703": 3, "SKU-711": 0}
  }
}
```

`grades` must contain every `development` and `held_out` intent; every graded document must exist
in the corpus; every `held_out` intent needs at least one document graded `2` or `3`; at least two
distinct graders must be named. `no_hit` intents need no grades.

## How the thresholds are evaluated

`tests/search/retrieval/search_relevance.rs` always runs each intent through Cassie SQL
(`WHERE search(body, $1) ORDER BY score DESC, _id ASC LIMIT 10`), checks that every `no_hit`
intent returns zero rows, checks the workload invariants, and writes per-query results to
`$CARGO_TARGET_TMPDIR/search_relevance_v1_results.json`.

The graded evaluation runs only when `judgments.json` declares `"status": "judged"`:

* nDCG@10 per held-out intent with gain `2^grade - 1` and discount `1 / log2(rank + 1)`,
  normalised by the ideal ordering of that intent's graded documents (0 if nothing is graded
  relevant).
* MRR@10 per held-out intent: reciprocal rank of the first document graded `2` or above in the
  top 10, else 0.
* The means over the `held_out` split must both be at least **0.80**. The thresholds are fixed
  constants in the harness and must not change during qualification.

While `status` is `unjudged`, the harness test asserts that the file is empty and well-formed and
the report records `"qualification": "pending_human_judgments"`: the relevance criterion is
therefore visibly **not satisfied**, not silently passed. The evaluation function itself is
unit-tested with synthetic judgments, including a decoy-first ranking that fails the thresholds.
