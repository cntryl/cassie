# Vector metric qualification

This finite qualification belongs to [#776](https://github.com/cntryl/cassie/issues/776)
and the existing [query-engine target](query-engine-target.md). It preserves finite
f32 VECTOR(n), single-node Midge storage and PostgreSQL wire as the primary query
interface. It introduces no type/OID, public API, persistent encoding or index
layout. Halfvec, sparsevec, quantization, bit operators, PostgreSQL operator classes
and pgvector ABI parity remain outside the selected target.

## Metric and path contract

L2 distance is Euclidean distance. Cosine distance is one minus similarity; if
either vector is zero, similarity is zero and distance is one, including
zero/zero. Dot distance is negative dot product, so a zero operand has zero
distance. The SQL scalar `dot_product` returns positive dot product. Components
must be finite representable f32; accumulation uses f64. Independent scalar
oracles use known zero, unit, opposite and orthogonal vectors, plus f32 subnormal
and maximum magnitudes at dimensions 1, 3, 4, 8, 9, 17 and 1025. Relative f64
rounding bounds apply to non-exact accumulated metric calculations, never to row
identities or candidate membership.

SQL operators execute generic scalar projection/ordering. Native SQL top-k selects
`vector_distance` L2, an identity projection, a matching ORDER BY expression and
a bounded LIMIT. SQL DESC retains exact fallback. Equality filters over eligible scalar indexes follow the [filtered vector contract](vector-filtered-search-contract.md). REST HNSW handles all three
metrics. REST IVFFlat handles L2 natively. Exact cosine can use complete normalized
sidecars. Exact dot scans the authoritative vector components, including on the
REST IVFFlat fallback path. Rounding normalized components to f32 can change
dot scores and their identity ties. [#1510](https://github.com/cntryl/cassie/issues/1510)
A five-vector fixture uses HNSW search capacity covering all candidates and IVF
probes covering all lists. Its independent exact ranking checks metric and identity
semantics; it does not promise arbitrary approximate-index recall. Runtime counters
prove native HNSW/IVF selection where claimed.

Missing/NULL vector fields are not retrieval candidates. Explicit valid vectors
remain candidates when the source field is absent or NULL. Normalized sidecars
are derived state: missing or partial sidecars cannot remove valid vectors.
Completeness uses the stored vector field's non-NULL population from hydrated,
generation-valid Midge statistics. Source-index cardinality counts source
membership and cannot establish vector population. Absent/stale statistics or an
incomplete normalized population decline to authoritative row scoring.
Warm entries also include that authoritative collection generation in their
private cache identity. Rebuilding current statistics after a same-count vector
update cannot reuse an older ranking, even when the catalog version is unchanged
or the normalized population exceeds the sentinel-check threshold. Inserting a
replacement prunes older generations of the same collection/field cache entry.

Ranking follows distance direction and then case-sensitive internal row identity.
A declared `id` remains an ordinary field; `_id` identifies the internal row.
Offset follows ranking. LIMIT 0 returns no rows; SQL retains dimension validation,
and REST does not invoke its embedding provider. The REST probe calls the actual
request adapter and controlled executor; transport framing remains owned by the
existing protocol/interface tests.

## Invariant disposition

New probes live in `tests/vector_embeddings/vector_metric_qualification.rs`; shared
fixtures and the independent oracle live in `tests/support/vector_qualification.rs`.
Existing evidence is reused by its focused subsystem owner.

| Invariant | Selected disposition and evidence |
|---|---|
| vec-001 | Dimensions rejected before zero-limit SQL across all three operators and text/native vector binds. Existing HNSW/operator dimension owners cover literal inputs. NULL is SQL NULL and is excluded from retrieval candidates; finite VECTOR(n) remains the #751 type boundary. |
| vec-002 | Independent f64 oracles cover subnormal/maximum opposite-sign components and SIMD tails; existing write/bind owners reject out-of-range and nonfinite components. |
| vec-003 | Independent distances and ranking cover positive/negative dot, antiparallel and orthogonal values. Existing scalar projection owners separately check positive `dot_product` and all operator signs. |
| vec-004 | Zero/zero and zero/nonzero checked across generic SQL, native L2 SQL, raw/normalized REST, native HNSW REST and L2 IVF REST; configured IVF cosine/dot retain documented exact fallback. |
| vec-005 | Missing/NULL mixed fixtures cannot enter retrieval rankings. Existing source lifecycle/write owners cover source NULL/omission and embedding clearing. Confirmed missing-sidecar defect for explicit SQL vectors fixed by vector-population completeness and authoritative fallback. A 1,025-vector warm-cache mutation witness preserves current ranking with unchanged population/catalog version, stale/missing statistics and a cold control. |
| vec-006 | Exact case-distinct identities, tied distances, ASC/DESC, offset and restart checked against independent ranking. Existing declared-id owner checks ordinary `id` versus `_id`. |
| vec-007 | Zero REST with unavailable query provider makes no provider call. SQL zero limit retains dimension validation. Offset/restart and at/beyond-cardinality witnesses preserve ranking windows; oversized SQL/REST windows fail without leaked reservations. |
| vec-008 | Different projection/ORDER BY query literals and direct operators match generic ordering and do not publish HNSW execution. Existing case-distinct sibling-field and alias owners preserve exact binder resolution. |
| vec-009 | Every new success/error probe releases query reservations. Existing exact budget and controlled normalized/HNSW/IVF memory/cancellation owners check atomic failure and success counters. Streaming exact top-k remains the authoritative bounded-heap boundary. |

## Evidence and close-out

Implementation baseline is `4a09c41a3670dad3a4c4c2b7ef7583327c37ab6b`.
The production red uses SQL INSERT of an explicit zero vector with a NULL source,
removes derived normalized sidecars and invokes REST search: baseline returns
zero rows when one valid vector exists. The passing regression also reads back
the stored SQL vector before REST execution. The fix
selects current vector-field statistics and declines incomplete normalized state.
The same regression passes alongside raw fallback, partial sidecars matching source
cardinality, missing/stale statistics and independent metric/ranking probes.

Independent review then reproduced a newly reachable warm-cache defect: 1,025
explicit vectors with NULL sources warmed normalized dot search; a direct Midge
vector update and same-count statistics rebuild retained the catalog version and
incorrectly reused the old ranking. The tracked regression returned `id0000`
instead of the cold-control winner `id0500`. The fix carries statistics'
`built_generation` in the private normalized cache key and preserves the existing
catalog and sentinel validity checks. No persistent format or public API changes.
The warm regression also exercises stale/missing-statistics fallback and verifies
released query reservations. Jev's 0.35 warm-generation judgment prompted this
focused reproduction and correction.
The current regression exercises cosine, which still uses normalized sidecars.

The isolated worktree retains exact commands, red and green logs, source hashes,
Jev requests/responses and the tested patch under ignored `target/`. Jev's initial
path coverage finding prompted native-path counter and raw-fallback witnesses;
its 0.70 cache-gap judgment prompted the positive-source/explicit-vector partial
sidecar witness. Those probes pass locally. Jev is review evidence, not correctness
proof.

After the generation refinement, the focused probe run passed 11/11 tests and
the controlled normalized/HNSW/IVF memory/cancellation selection passed 5/5.
The exact existing vector owner selection passed 19/19 on the earlier population
completeness patch retained as `target/776-tested.patch`; that run is not claimed
as a final-generation-patch rerun. The final focused review judged the corrected
sequential generation gap at 0.12 and selected no additional supported probe.
These are focused local checks, not the full gate sequence.

The build and full locked suite passed at
`0a597dbf748814fd7e2f46558499784cf52ae934`: 3,372 passed, zero failed and nine
existing ignored tests across 32 groups. Subsequent shared pedantic idioms,
equivalent empty-result diagnostics and test-name refinements produced
`b3ea99234c2a291f361ec7af9afe888c16f280ac` without changing the vector runtime
source or the qualification cases. The full suite was not rerun at that revision.
Its refinement sequence passed build, 20 typed aggregate tests, 25 typed batch
tests, 11 vector qualification tests, full workspace/all-target/all-feature
pedantic Clippy, format, the three touched-test policy checks, and documentation,
benchmark and module-size policies.

Evidence inspection found that the original source-join filter selected zero
tests; its successful exit is not counted as coverage. A supplemental corrected
filter, `executor::execution::source::source_join::`, then passed all 39 source-join
tests at the unchanged refinement revision. This supplement ran after the policy
gates and changed no source. Actual focused coverage therefore totals 95 tests.
Exact commands, source and log hashes, prior failed/canceled runs and the
refinement provenance remain in
`target/independent-776-review/final-publication-manifest.json`.

Final publication-base equivalence, visible review, exact publication-head hosted
checks, squash merge and source/issue readback remain pending. #776 stays open
until those steps and target-selected acceptance are complete. This record adds
no broader feature/readiness promotion.

## Hosted controlled-read fixture correction

PR #856's first hosted core run at `1281e0b2` passed 580 unit tests and failed
one existing ordered-read ResourceLimit canary with QueryCanceled instead.
The exact hosted interleaving is unknown. A deterministic, source-snapshot
probe proved the mechanism: while an owner armed cancellation at its second
read, an unguarded unrelated thread consumed the global first read, causing the
owner's first hook to cancel. New production ownership and late-worker tests
were genuinely red before the private scope repair; a separate unguarded
rearm test exposed and corrected stale scope retention after disarming.

The private canary now belongs to a fixture scope captured in the existing
cancellation handle. Handle/control clones preserve that scope through blocking
workers and uncapped inputs; old workers cannot consume a later fixture's
canary. Original cancellation identity remains the same AtomicBool Arc, and
ordinary unarmed queries retain their existing cancellation/read behavior.
The obsolete global fixture lock is removed. Guards retain their thread-local
nesting and non-Send lifecycle; guard drop disarms only its own scope.

Test fixtures must acquire their guard before creating query handles or
controls, allowing handles constructed before arming to capture ownership.
Legacy unguarded use creates handles after arming and discards the scope on
disarm; it does not preserve the previous process-wide cancellation behavior.
The existing setter/guard public signatures stay unchanged. All existing
repository setter callers either acquire a guard directly or inherit it from
an explicitly guarded test; the new unguarded lifecycle probe is separate.

Focused controller, ordered-row/row-store resource canaries, retrieval and
analytical cancellation paths, pgwire SQLSTATE/portal controls and the actual
REST blocking-worker boundary retain their exact read-count and cleanup
assertions. The coherent correction requires a new complete ordered local
validation and fresh exact-head hosted checks; the earlier 3,372-test result
is prior provenance, not completion of this behavioral fixture correction.
Exact source snapshots, red/green logs, Jev gap probes, canceled diagnostics
and final gates are retained under `target/independent-776-review/`.

## Metric-matched ANN for SQL operators (#1362)

This section owns the SQL ANN contract for the pgvector operators. It adds no
type, OID, public API or persistent encoding.

### Operator, metric and index contract

| SQL operator | Metric | Distance (ascending is nearest) | Matching index metadata |
|---|---|---|---|
| `<->` | `l2` | Euclidean distance | `metric = l2` |
| `<=>` | `cosine` | `1 - cos`; zero operand gives `1` | `metric = cosine` |
| `<#>` | `dot` | negative inner product | `metric = dot` |

An index is created with `CREATE INDEX ... USING vector (field) WITH (source_field
= ..., metric = l2|cosine|dot, index_type = hnsw|ivfflat, ...)`. The `metric`
option is the operator class: there are no pgvector `vector_*_ops` names.

A statement is served from an HNSW or IVFFlat index only when all of these hold:

- Shape: `SELECT <column>, <field> <op> <query> [AS d] FROM t ORDER BY d |
  <field> <op> <query> LIMIT n [OFFSET m]`, with the same operator, field and
  query in projection and ORDER BY, and `<column>` an ordinary (non-identity)
  column. The `vector_distance()` function shape is unchanged and is always L2.
- No `WHERE` predicate, no open transaction overlay on the collection, ascending
  order, and a bounded `LIMIT`.
- The index metric equals the operator metric.

Candidates come from the index; every candidate is then re-read from the source
row and re-scored exactly with the operator's metric. Ranking is exact distance
ascending, then case-sensitive row identity; OFFSET follows ranking. Rows whose
vector is missing or NULL are never index candidates. The REST vector search is
unchanged: HNSW serves all three metrics and IVFFlat serves L2 only.

### Diagnosed exact fallbacks

Each fallback returns the exact result and increments the algorithm's
`*_fallbacks` counter; `vector.last_fallback_reason` carries the reason:

| Reason | Condition |
|---|---|
| `incompatible-metric` | index metric differs from the operator metric |
| `unsupported-sort` | `ORDER BY ... DESC` |
| `zero-query-exact` | cosine or dot query is the zero vector: every distance ties, so the order is row identity alone and cannot be reproduced from a candidate set |
| `missing-graph` / `missing-training` | HNSW graph or IVFFlat manifest is absent or invalidated |
| `missing-list-bounds` | IVFFlat manifest lacks the per-list inner-product bounds a `dot` probe needs |
| `candidate-exhausted` | fewer candidates than `min(limit + offset, rows)` (for example IVFFlat with too few probed lists) |
| `concurrent-source-change` | the source row set changed between candidate selection and rerank in a way the captured snapshot cannot serve |
| `structured-filter-exact`, `transaction-overlay-exact` | existing filtered / transactional fallbacks |

`EXPLAIN` of an eligible-shape statement appends
`vector_ann=<eligible|incompatible-metric|unsupported-sort|structured-filter-exact|no-ann-index>
vector_ann_index=<hnsw|ivfflat|bruteforce|none> vector_ann_index_metric=<metric>
vector_ann_query_metric=<metric>`. It reports plan-time eligibility; run-time
fallbacks (`zero-query-exact`, exhaustion, missing manifest) appear only in the
counters above. A reader paused between candidate selection and rerank keeps its
captured source snapshot and still reports an ANN execution; the next statement
sees the committed mutation.

### Executable evidence

`tests/vector_embeddings/vectors/sql_ann_metrics.rs`,
`sql_ann_metric_lifecycle.rs` and `sql_ann_metric_recall.rs`:

- Both algorithms x L2/cosine/dot against an independent f64 oracle on a
  30-row non-unit fixture: identity, distances (1e-9), execution counter delta of
  exactly one, positive exact-rerank delta, zero fallbacks.
- A planted row is the cosine best match but not the Euclidean nearest; an
  L2-only plan mislabelled as cosine fails the oracle.
- Metric mismatch (3 pairs x 2 algorithms), DESC, zero query, invalidated
  manifest, deletion and same-count vector replacement between candidate
  selection and rerank, candidate exhaustion, OFFSET, and EXPLAIN reporting.

### Recall@20 measurement

Recall is `|ANN top-20 ∩ exact top-20| / 20`, exact top-20 being the f64 oracle
(distance, then identity). Fixture: deterministic modular 3-D vectors,
row `i` = `[(7i mod 101)/101, (13i mod 103)/103, (29i mod 107)/107]` mapped to
`[-1, 1)`; ten fixed queries (see the test file); in-memory storage, native SQL
path, `LIMIT 20`, floor `0.90`. Settings are the smallest profiles declared in
`ivfflat-release-evidence.md` and `hnsw-release-evidence.md`. The queries are
the tuning set; no held-out split was used and no parameter was tuned.

| Algorithm | Rows | Settings | Metric | Mean recall | Worst query |
|---|---:|---|---|---:|---:|
| HNSW | 1,024 | m=32, ef_construction=256, ef_search=256 | l2/cosine/dot | 1.0000 each | 1.00 each |
| HNSW | 10,000 | m=32, ef_construction=256, ef_search=256 | l2 | 1.0000 | 1.00 |
| HNSW | 10,000 | same | cosine | 1.0000 | 1.00 |
| HNSW | 10,000 | same | dot | 1.0000 | 1.00 |
| IVFFlat | 1,024 | lists=16, probes=4, sample=1024, seed=42 | l2 | 1.0000 | 1.00 |
| IVFFlat | 1,024 | same | cosine | 1.0000 | 1.00 |
| IVFFlat | 1,024 | same | dot | 0.9900 | 0.95 |

Per-query IVFFlat dot recall: `1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.95, 1.0,
0.95`. Every metric/algorithm row meets the 0.90 floor on every query, and
`sql_ann_metric_recall.rs` asserts the per-query worst case, not the mean.

**IVFFlat inner-product probing.** The first measurement (probes ordered by the
normalized query direction) gave mean 0.9050 and worst 0.55
(`0.55, 1.0, 1.0, 1.0, 0.75, 0.85, 1.0, 0.9, 1.0, 1.0`). A probe of the
failing queries confirmed the cause: IVFFlat lists are directional cells (the
centroids are sampled unit vectors, members are assigned by L2 on normalized
vectors), so each list mixes magnitudes and ranking lists by direction, or by
`query . centroid`, selects the same four lists. For example query 0 held its
true top-20 in lists 1, 10 and 12 but probed 0, 1, 10 and 11, missing the 9
results in list 12. The fix ranks lists for a `dot` query by the best inner
product each can still hold, `query . mean + |query| * radius`, where `mean`
is the mean of the list's members at their original magnitude and `radius` is
the largest member distance from it (a Cauchy-Schwarz bound). The manifest
stores `[mean..., radius]` per list (`list_bounds`), derived with the
assignments at training time, so training and probing stay consistent. L2 and
cosine probing and the exact rerank are unchanged; there is no format
compatibility with earlier manifests (pre-release).

Orders measured on this fixture and its ten queries at lists=16, probes=4
(mean / worst): direction 0.905 / 0.55; a per-list max-magnitude angular-cap
bound 0.885 / 0.70 (implemented, then dropped); `query . mean` alone 0.985 /
0.85; mean-plus-radius bound 0.990 / 0.95 (shipped). The queries are shared
across these comparisons, so the shipped order was chosen among four
candidates on the same queries; no parameter was tuned. Even an oracle list
order (best member per list) only reaches 0.95 on query 7, so probes=4 of 16
lists bounds attainable recall at this profile.

The 1,024-row HNSW rows run in the suite by default; the 10,000-row HNSW rows
were measured once with `CASSIE_ANN_RECALL_HNSW_ROWS=10000` (about 60 s of index
build; under heavy machine load a 10,000-row build once hit the default 30 s
query timeout, so rerun it on an idle machine). The 100k, 768-dimension and
filtered profiles belong to #1365 and #1363 and were not run here. Timing was
not measured.
