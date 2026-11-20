# Recoverable Vector Index Publication

SQL and REST sourced-vector index creation use the existing pending-index publication
journal. This contract extends that journal; it does not add another storage layer or
change authoritative row encoding. Legacy pending records without vector backfill
continue through their existing replay path.
Older binaries do not understand vector backfill intents. Complete recovery with a
version that implements this contract before downgrading a database with pending work.

## Preparation

Hold the collection and referential write gates while scanning existing rows. Preserve
explicit vectors and skip NULL source values. Generate every required embedding before
publication, then prepare each new payload with schema, CHECK, uniqueness and foreign-key
validation, excluding its own row identity. A statement-local overlay makes earlier
prepared rows visible to later validation. Provider or validation failure leaves base
rows and index metadata unchanged and creates no publication intent.

## Journal Version 1

The existing `PendingIndexPublication` record gains an optional `vector_backfill`
object. Its version is `1`; unknown versions fail closed. The object contains the
vector index metadata, a unique publication ID, the prepared-row count, source
collection generation, a SHA-256 digest of the ordered length-framed staged row records, whether SQL index metadata is also published, and the normal
rollup/materialized-projection maintenance-debt flags.

Prepared row payloads are separate owning-database-family records keyed by publication ID and
row ordinal using the existing LexKey encoding. Stage at most 5,000 rows per database-family
transaction, then persist the pending intent only after every row record is durable.
Unreferenced staging records are disposable and are removed during startup cleanup before request admission.
Missing, malformed, duplicate-identity, digest-mismatched or extra staged rows fail closed.

## Publication And Recovery

1. Verify the captured collection generation before applying prepared payloads.
2. Commit all prepared rows through the normal document batch path in one database-family
   transaction. An applied marker carrying the publication ID is committed in that same
   transaction, along with collection generation, data epoch and derived maintenance
   debt. Thus a storage failure cannot leave a partially applied backfill.
3. Build generation-bound vector sidecars with the existing bounded build transactions.
   Prepared embeddings are reused; replay never contacts the embedding provider.
4. Atomically publish vector metadata, optional SQL index metadata, and remove the
   pending intent in one owning-database-family transaction. Register runtime catalog metadata
   before releasing the collection write gate.
5. Delete staging records and the applied marker after publication. Interrupted cleanup
   is harmless and retried on startup.

A failure before the intent is durable leaves rows unchanged. A publication failure after durable
intent and before index metadata commits returns an explicit recoverable-publication
error naming the collection; the intent remains. If data publication committed, reads may observe the fully validated
backfilled rows even though index creation returned an error. This is tracked recovery
work, never a successful DDL response or an untracked partial mutation. Invalidate
execution-result caches on publication errors; when the durable data epoch advanced,
install that epoch, mark dependent projections stale and synchronize derived maintenance
debt into the runtime catalog before releasing the write gate.

Startup replays the intent before catalog hydration. An absent applied marker means
the data transaction did not commit; a present marker means it committed completely.
Recovery checks the expected generation and prepared payloads before proceeding, comparing
their canonical row-encoding bytes so valid f32 JSON round trips do not appear changed.
If ordinary writes or DDL changed the collection after an interrupted attempt, recovery
fails closed rather than overwriting newer data or publishing stale sidecars. Preserve
the data directory and journal, stop writes, and restore the last known consistent
snapshot or investigate the reported mismatch before retrying startup. Do not delete
journal records to force startup.

SQL and REST must satisfy the same provider-failure, constraint-failure, atomic-write,
interrupted-build, generation-mismatch, restart and repeat-replay invariants. Existing
query behavior is unchanged, and Cassie remains pre-release.

## Repeated Cold Startup Qualification

`tests/vector_embeddings/publication_restart.rs` owns two local, two-row witnesses:
`should_recover_prepared_sql_vector_publication_across_two_cold_startups` interrupts
SQL creation after preparation and before Data application;
`should_recover_data_applied_rest_vector_publication_across_two_cold_startups`
interrupts REST creation after the atomic Data commit and before sidecar publication.

Each witness inspects the durable intent, exact staged identities and ordinals,
and the publication-ID marker before shutdown. The prepared branch requires
unchanged complete row payloads, canonical row bytes, collection generation and
Data epoch, with no applied marker. The applied branch requires every staged
payload, the exact marker, and a single generation and epoch increment, while
vector and SQL metadata remain unpublished.

Each subsequent cold startup must recover complete payloads, preserve canonical
row bytes, hydrate the exact vector and SQL catalog metadata, and finish with
no pending intent, staging records or applied marker. Those read-only assertions
run before any manual replay. SQL creation publishes its SQL index; REST creation
keeps SQL index metadata absent. Provider call counts stay zero, and generation
and Data epoch advance only once across both startups. Final directory removal
requires all engine, session and provider owners to have retired.

The finite [#899](https://github.com/cntryl/cassie/issues/899) extension adds
read-only assertions at both cold boundaries for the existing brute-force,
cosine, three-dimensional profile. It requires exactly two physical and decoded
normalized records with the staged physical identities, complete metadata and
recovered generation. Magnitudes and coordinates must match an independent
scalar equation over the staged f32 vectors, including finite and represented-bit
checks. Exactly one physical vector-state record must decode to the recovered
generation with no HNSW graph or IVFFlat training state. Raw and decoded snapshots
must match across both startups.

Execution snapshot, 2026-10-10: at source `fa2f2702`, both existing witnesses
passed in hosted storage qualification, with byte correspondence to the tested
merge recorded in [the source and execution review](https://github.com/cntryl/cassie/pull/900#pullrequestreview-5479848665).
An additional disposable assertion-sensitivity probe inserted one malformed
physical record while decoded records remained unchanged. The existing physical
count assertion rejected it, exact raw and decoded state was restored, and the
original second startup and strict cleanup completed; see
[the probe review](https://github.com/cntryl/cassie/pull/900#pullrequestreview-5480040061).
At this snapshot, complete ordered local validation and final acceptance remain
pending. Later immutable-head validation and integration records live in PR #900;
these finite witnesses do not qualify the wider parent matrix.

These witnesses add bounded evidence for `pub-004`, provider reuse in `pub-005`,
and startup cleanup in `pub-006` under
[#764](https://github.com/cntryl/cassie/issues/764). The broader ANN, corruption,
concurrent generation, maintenance, lifecycle and native release matrix remains
tracked there; #764 and its downstream #765 qualification stay open.

## Orphan-Only Cold Startup Qualification

The finite [#918](https://github.com/cntryl/cassie/issues/918) witness qualifies
startup with no pending intent, two unreferenced Schema staging records and one
Data applied marker. One staged value is opaque invalid JSON: the existing
orphan-disposal rule removes unreferenced keys without decoding their values;
referenced malformed staged rows retain their fail-closed contract.

`tests/vector_embeddings/publication_orphans.rs` must prove exact durable
artifact presence before the first startup and absence immediately after each
of two cold startups, before any manual replay or cleanup. Complete row payloads,
canonical row bytes, collection generation and Data epoch must remain unchanged.
No vector or SQL metadata may appear, and provider calls must remain zero. The
second startup must be idempotent, with all owners retired before directory
removal. The two existing live-intent recovery witnesses remain unchanged as
published-state controls.

Execution record, 2026-10-11: the orphan witness and both unchanged live-intent
controls passed in [hosted storage qualification](https://github.com/cntryl/cassie/actions/runs/38088951196/job/114321247347)
at tested merge [`65dcae93`](https://github.com/cntryl/cassie/commit/65dcae93c16ba3ae2e81fe101c6d446ba48d2110),
for PR head [`6c2cde77`](https://github.com/cntryl/cassie/commit/6c2cde77c9c747a37ae181a0dc14d1e07bf45721).
[PR #920](https://github.com/cntryl/cassie/pull/920) tracks the separate
retained-executable startup probe, complete ordered local validation and final
integration status. This finite witness adds no format, API or support promotion;
broader parent #764 acceptance and downstream #765 qualification remain open.

## Rebuild Publication And Churn Qualification

Whole-index rebuilds (index creation, `REPAIR INDEX`-style refresh, startup
reconciliation) no longer stage every artifact in one transaction. Midge
validates each detached transaction against an 8 MiB budget that charges 512
bytes plus key and value size per operation, so a single-transaction rebuild of
a few thousand wide vectors fails. Artifacts are now staged in transactions
bounded by estimated bytes (4 MiB) and operations (5,000), diffed against the
persisted artifacts (unchanged records are skipped; obsolete graph nodes, list
memberships and normalized vectors are deleted), and the source summary and
state manifest are written together in the final transaction. A reader that
sees a partially staged rebuild finds a manifest that no longer matches its
artifacts and takes the labelled exact fallback; the manifest never points at
unstaged artifacts. HNSW document writes no longer rebuild the graph; see
[HNSW write maintenance](#hnsw-write-maintenance). IVFFlat writes still
recompute list membership inside the writing transaction.

### HNSW write maintenance

A committed document write maintains each HNSW index of its collection for the
changed documents only, inside the writing transaction. Nothing proportional to
the unrelated collection is scanned, hydrated, read or rewritten.

- **Insertion** follows the standard algorithm: a deterministic per-identity level
  from the same level distribution a rebuild uses, greedy descent from the entry
  point, an `ef_construction` search per layer, nearest-first neighbor selection,
  and bidirectional links pruned to `m` closest. The entry point moves to a new
  node that is taller than the graph.
- **Links are symmetric.** A neighbor that overflows `m` drops its farthest link
  and that node drops the link back. A full build keeps the same invariant, so
  the in-links of any node are exactly the nodes in its own lists.
- **Update** of a changed vector is a delete followed by an insert. An unchanged
  vector writes nothing.
- **Deletion** unlinks the node from each neighbor on every layer, reconnects
  the orphaned neighbors to one another when both have room (nearest first), and
  deletes the node record. A deleted entry point is replaced by its tallest
  neighbor on its highest populated layer, which also resets `max_layer`.
- **Cost.** Only visited nodes are point-read from the transaction. Written
  records are the new or changed nodes plus the manifest (entry point, layer,
  row count, source fingerprint) and the source summary. The source fingerprint
  is folded in and out per record instead of recomputed, so it is an opaque
  change token that readers compare only between manifest and summary.
- **Normalized vectors** are restamped for changed documents only. Unchanged
  records keep the generation they were last written at, so the sidecar fence
  accepts a record stamped between the generation of the last full publication
  (persisted as a floor record beside the field's normalized vectors) and the
  generation observed by the reader. A rebuild rewrites every record and
  advances the floor; without a floor, equality with the observed generation is
  still required.

A write falls back to a full rebuild, never to a failed write, when the persisted
graph cannot be trusted: no or undecodable manifest, an older generation than the
write expects, a different metric, dimension count or graph version, a missing or
undecodable node, a neighbor too short for its layer, an entry point that cannot
be re-elected, or a batch above 128 documents. A rebuilt graph estimated at 256
KiB or less is written inside the commit. A larger one is unpublished in the
commit (manifest and source summary deleted, readers take the labelled exact
fallback), recorded as durable `vector:<field>` maintenance debt, and rebuilt in
bounded transactions as soon as the commit is durable; startup retries any debt a
crash left behind. The debt is cleared when the rebuild publishes.

Counters in the `vector` metrics section: `index_maintenance_commits_total`,
`index_maintenance_documents_total`, `index_maintenance_nodes_read_total`,
`index_maintenance_nodes_written_total`, `index_maintenance_nodes_deleted_total`,
`index_maintenance_full_rebuilds_total` (in-commit fallbacks),
`index_maintenance_deferred_rebuilds_total`, and the
`index_maintenance_last_nodes_{read,written,deleted}` values of the latest commit.

Contract: the incremental graph need not equal a fresh build, but it must pass
`cassie::vector::hnsw::graph_validity_violation` (existing neighbors tall enough
for their layer, symmetric links, at most `m` links, entry point on the highest
layer) and keep recall@k at or above the declared 0.90 floor. Reads still exact
rerank from source rows, so the index only affects candidate recall.

### Generation labels and stale attempts

A statement reads its index state, candidates and rows from one snapshot.
`vector.last_ann_observed_generation` is the collection generation of the latest
successful ANN answer, recorded after the final source-generation check. A
reader paused at rerank while a write commits is therefore labelled with the
generation it read, never the newer one. If an attempt does observe an
inconsistency (a missing candidate row, malformed vector or generation
mismatch) it is labelled `concurrent-source-change`, increments
`vector.ann_stale_attempt_discards_total` and the algorithm fallback counter,
records no execution counters, and the exact path runs once. A stale persisted
manifest (`concurrent-source-change` for both algorithms) takes the same
labelled fallback instead of failing the query.

### Operator procedure

1. Detect: `vector.hnsw_fallbacks` or `vector.ivfflat_fallbacks` rising, with
   `vector.last_fallback_reason` of `concurrent-source-change`, `stale-graph`,
   `missing-graph`, `missing-training` or `stale-*`, means queries are exact.
   `vector.ann_fallback_micros_total` and `ann_last_fallback_micros` give the
   time of function-shaped (`vector_distance`) statements answered exactly for an
   indexed field; operator-shaped (`<->`) statements fall through to the generic
   scan and are timed by query latency.
2. Rebuild: run the index repair statement for the collection, or restart
   (startup reconciles IVFFlat and rewrites normalized vectors). Progress is
   visible in `vector.index_rebuilds_total`, `index_sidecar_transactions_total`
   and `index_sidecar_bytes_written_total`; when it finishes,
   `index_last_rebuild_micros`, `index_last_rebuild_bytes_written` and
   `index_last_rebuild_transactions` describe it.
   `index_max_sidecar_transaction_bytes` stays at or below 4 MiB.
3. Confirm return to indexed service: a fresh eligible query increments
   `hnsw_executions` or `ivfflat_executions` with no fallback increment, and
   `last_ann_observed_generation` equals the collection generation. A reopen
   selects the same published generation. Interrupting a rebuild before its
   final transaction leaves the previous manifest; interrupting after it leaves
   the new one; either reopens usable, and a retry reclaims obsolete artifacts.
4. Persisted size: the `vector.index_*` counters and the
   `vector_index_artifact_footprint_for_diagnostics` diagnostic (key and byte
   counts per artifact family) bound the extra persisted bytes. Peak query
   memory is `query.peak_accounted_memory_bytes`.

### Evidence

`tests/vector_embeddings/indexes/churn/incremental_maintenance.rs` owns the write-cost
bound (nodes written for one document stay under a fixed bound at 60, 600 and
10,000 rows), seeded insert, replace and delete churn that validates the graph after
every step including entry-point deletion, the recall floor after churn, exact-rerank
results after churn, emptying and repopulating a collection, and repair of a damaged
graph in the commit and after it. `incremental_restart.rs` reopens an incrementally
maintained graph on local disk.

`tests/vector_embeddings/indexes/churn/` owns the seeded churn sequence (insert,
replace-vector-with-same-count, delete, aborted write, rebuild) over HNSW and
IVFFlat; the paused-reader, publication-pause and rebuild-pause barrier tests;
local-disk interruption before and after manifest publication with reopen,
stale-state fallback and obsolete-artifact reclamation (asserted by key counts
under the index prefixes); and the larger-than-one-transaction build.
`benches/support/vector_index_churn.rs` (opt in with
`CASSIE_BENCH_VECTOR_INDEX_CHURN=1` on `tier5_scaling_retrieval`) prints build
time, rebuild time and bytes, fallback duration, return-to-indexed-service time,
persisted index bytes and peak query memory per algorithm and scale. Measured
values are not recorded here until the profile runs on the pinned fixture.
