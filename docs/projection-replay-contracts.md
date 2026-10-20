# Projection Replay Contracts

Cassie replay is a local read-model ingestion contract. The source event stream, event ordering policy, retry policy, and projection handler code remain application-owned. Cassie owns local projection rows, replay checkpoint metadata, duplicate detection, freshness state, metrics, and catalog diagnostics.

This surface is Cassie-specific and part of the Stable local projection lifecycle in [Feature Support](feature-support.md). Stability is limited to the documented single-instance replay contract; it does not establish deployment-level production readiness.

## Ownership Boundary

| Area | Cassie owns | Application handler owns |
| --- | --- | --- |
| Source stream | No remote stream storage, polling, offsets, or retention | Durable event log, stream partitioning, source retention, event ordering |
| Event ordering | Rejecting out-of-order positions for a projection/source pair | Delivering events in deterministic source order |
| Idempotency | Duplicate ledger keyed by projection, source identity, and event id | Stable event ids that uniquely identify source events |
| Batch identity | Persisting `replay_batch_id` for diagnostics | Choosing batch boundaries and retry cadence |
| Payload shape | Normal Cassie schema, constraint, index, and row-write validation | Deterministic handler mapping from event payload to projection document |
| Schema versions | Projection metadata and validation against current Cassie schema | Event schema compatibility, upcasters, and handler version rollout |
| Time and generated values | Persisting values provided in replay payloads | Resolving timestamps, generated ids, random values, and other non-deterministic functions before replay |
| Failures | Failed freshness, `last_error`, metrics, and restart hydration | Handler retry decisions, source replay window selection, and external alerting |

Cassie must not be treated as the source event store. A Cassie snapshot or projection checkpoint is recovery evidence for local projection state, not a replacement for the application event stream.

## Deterministic Handler Expectations

- Events for one `(projection, source_identity)` pair must be replayed in monotonically increasing source position when positions are present.
- `event_id` must be non-empty, stable across retries, and unique for each source event within the projection/source pair.
- The caller must not acknowledge the source position or submit a later batch after a replay error; retry the same event batch until its durable checkpoint is confirmed.
- Redelivery of an event whose Schema-family ledger commit completed is idempotent: Cassie skips it and increments duplicate skip diagnostics without rewriting projection rows.
- If projection rows committed in the Data family but the Schema-family replay-state transaction did not, the event has no committed ledger entry. Retrying the same batch may reapply its deterministic document replacements/deletions before committing the ledger and checkpoint.
- The same new event id appearing more than once in a single batch is a replay conflict. Cassie fails the batch before applying projection row writes.
- `batch_id` is diagnostic metadata. It is not the idempotency key and must not be used as a source offset.
- Replay payloads must already contain deterministic timestamps, generated ids, and any values derived from clocks, random sources, sequence generators, or external services.
- Handler schema-version decisions must happen before replay. Cassie validates the resulting document against the current projection schema, but it does not run event upcasters or handler migrations.

## Failure Handling

Replay failures are observable through `pg_catalog.pg_projection_checkpoints`, projection metrics, and the `ProjectionReplayReport`/error returned by the local API.

Cassie records failed freshness and `last_error` for:

- projection/source identity mismatch
- empty event ids
- duplicate new event ids inside one batch
- out-of-order event positions
- row, schema, constraint, index, or storage write errors
- replay-state persistence errors when the failure is known before commit

For preflight conflicts such as empty event ids, out-of-order positions, and duplicate event ids inside a batch, Cassie does not apply projection row writes and does not advance `source_checkpoint` or `last_applied_event_id`.

Replay crosses two Midge transaction boundaries. Cassie first commits the row and index writes in the Data family. It then commits the duplicate-ledger entries and checkpoint metadata together in one Schema-family transaction. Cassie never opens a transaction across those families. Rows can therefore be visible while the previous checkpoint is still stored. If the Schema-family transaction does not commit, neither its event ledger entries nor its checkpoint update is visible; replaying the same deterministic batch applies the row writes again and retries that transaction. If the caller receives an uncertain commit error, the durable ledger/checkpoint state is authoritative and retrying the same batch resolves the outcome. A successful Schema-family transaction cannot leave a new ledger entry paired with an old checkpoint.

Preflight and row-write failures persist failed freshness when the failure is known before the checkpoint commit. Failed metadata is hydrated on restart so operators can see that the projection is not fresh. A failure returned by the final Schema-family commit may have an uncertain outcome; inspect durable checkpoint state and retry the same batch instead of assuming either commit result.

## Operator Workflow After Replay Failure

1. Inspect `pg_catalog.pg_projection_checkpoints` for `freshness`, `last_error`, `replay_batch_id`, `source_checkpoint`, and `last_applied_event_id`.
2. Fix the handler, input ordering, schema mapping, or source replay window outside Cassie.
3. Resume replay from the last committed source checkpoint or rebuild the projection from the durable source event stream.
4. Run verification before trusting a rebuilt or repaired projection.
5. Swap or activate a materialized projection version only after replay is fresh, verification succeeds, and the active-version workflow reports a safe state.

Do not swap an active projection version after a failed replay solely because a batch retry completed locally. Verification and fresh checkpoint state are the operator guardrails.

## Evidence

- `tests/analytics.rs` module `projection_lifecycle` covers idempotent duplicate skip, out-of-order failed freshness, duplicate event ids inside one batch, restart hydration of failed replay metadata, retry after a committed Data-family write with an aborted Schema-family transaction, and retry after a Schema-family commit whose error response left the caller uncertain.
- `docs/production-readiness.md` records the Stable local projection lifecycle promotion and its retained native-disk replay, refresh, verification, and repair evidence. Deployment-profile alert thresholds remain open under #8 and are not implied by that promotion.
