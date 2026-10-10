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

Prepared row payloads are separate schema-family records keyed by publication ID and
row ordinal using the existing LexKey encoding. Stage at most 5,000 rows per schema
transaction, then persist the pending intent only after every row record is durable.
Unreferenced staging records are disposable and are removed during startup cleanup before request admission.
Missing, malformed, duplicate-identity, digest-mismatched or extra staged rows fail closed.

## Publication And Recovery

1. Verify the captured collection generation before applying prepared payloads.
2. Commit all prepared rows through the normal document batch path in one data-family
   transaction. An applied marker carrying the publication ID is committed in that same
   transaction, along with collection generation, data epoch and derived maintenance
   debt. Thus a storage failure cannot leave a partially applied backfill.
3. Build generation-bound vector sidecars with the existing bounded build transactions.
   Prepared embeddings are reused; replay never contacts the embedding provider.
4. Atomically publish vector metadata, optional SQL index metadata, and remove the
   pending intent in one schema-family transaction. Register runtime catalog metadata
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
query support labels and Cassie's Production Candidate status are unchanged.

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
must match across both startups. These new assertions have not yet executed;
their qualification remains pending until focused and full validation complete.

These witnesses add bounded evidence for `pub-004`, provider reuse in `pub-005`,
and startup cleanup in `pub-006` under
[#764](https://github.com/cntryl/cassie/issues/764). The broader ANN, corruption,
concurrent generation, maintenance, lifecycle and native release matrix remains
tracked there; #764 and its downstream #765 qualification stay open.
