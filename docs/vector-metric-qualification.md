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
a bounded LIMIT. SQL DESC retains exact fallback. REST HNSW handles all three
metrics. REST IVFFlat handles L2 natively; cosine/dot use normalized exact fallback.
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
Exact commands, source and log hashes, prior failed/cancelled runs and the
refinement provenance remain in
`target/independent-776-review/final-publication-manifest.json`.

Final publication-base equivalence, visible review, exact publication-head hosted
checks, squash merge and source/issue readback remain pending. #776 stays open
until those steps and target-selected acceptance are complete. This record adds
no broader feature/readiness promotion.
