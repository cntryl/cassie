# Projection Replay Contracts

Cassie replay is a local read-model ingestion contract. The source event stream, event ordering policy, retry policy, and projection handler code remain application-owned. Cassie owns local projection rows, replay checkpoint metadata, duplicate detection, freshness state, metrics, and catalog diagnostics.

This surface is Cassie-specific and part of the local projection lifecycle in
[Feature Support](feature-support.md). The selected atomic replay change is
not yet qualified by the historical Stable lifecycle evidence. Issue #1270
tracks its complete local, recovery and hosted qualification; deployment-level
production readiness remains separate.

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
- Redelivery of an event whose durable ledger commit completed is idempotent: Cassie skips it and increments duplicate skip diagnostics without rewriting projection rows.
- Projection rows, their transactional derived maintenance, duplicate ledger and checkpoint commit together in one owning-family Midge transaction. Replaying a batch after a known pre-commit failure applies it once; after an unknown outcome the caller retries the same deterministic batch and the recovered ledger determines whether it committed.
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

Replay publishes rows, transactional index mutations, event ledger entries, and checkpoint metadata in one transaction in the owning database family. A committed event's row mutations and checkpoint are visible together in the same storage snapshot. If the transaction aborts, neither its row mutations nor its ledger or checkpoint updates are visible. This includes a new event whose deletion targets a missing row: the ledger and checkpoint must still commit. Duplicate-only and empty batches still commit their replay metadata and diagnostics. A preparation based on a stale checkpoint or duplicate-ledger view fails with a retryable conflict; the caller retries the whole batch after reading current state. The in-process collection gate is not a substitute for storage transaction conflict detection. Cassie does not automatically retry an uncertain commit outcome; the caller reconciles the recovered ledger and retries the same deterministic batch.

The stored row, ledger and checkpoint share an atomic Data-family commit.
`pg_catalog.pg_projection_checkpoints` currently reads projection metadata from
the live catalog, however, so a SQL statement that joins it to rows pinned by
an earlier Data snapshot can observe different replay generations. Do not use
that virtual view and a projection-row query as a same-snapshot consistency
check. [#1272](https://github.com/cntryl/cassie/issues/1272) tracks the public
view's snapshot, transactional DDL, retained portal and resource-control work.
Until that work is complete, inspect recovered persisted replay metadata when
reconciling the durable checkpoint and event ledger.

Preflight and known precommit row-write failures attempt to persist failed freshness, guarded by the metadata version read during preparation. A changed checkpoint returns conflict rather than overwriting newer metadata. A storage stall preserves its original retryable error if the separate diagnostic write also fails. Failed metadata is hydrated on restart when that diagnostic commit succeeds; durable failure diagnostics cannot be promised when storage is unavailable. A fenced writer must not submit another diagnostic write. A failure returned after commit may have an uncertain outcome; inspect recovered checkpoint and ledger state and retry the same batch instead of assuming either commit result. In particular, a failed metadata reread after a successful commit can return an error while the rows, ledger and checkpoint are already committed and the live catalog remains stale. A same-batch retry reconciles duplicate events when storage is available. The caller owns its external source acknowledgment. An external offset can survive loss of the entire Cassie database: recovery must rewind or rebuild from retained source events rather than trusting an offset beyond recovered Cassie state. This contract does not change the selected Midge write policy or establish crash atomicity for a new durability policy.

## Operator Workflow After Replay Failure

1. Inspect `pg_catalog.pg_projection_checkpoints` for `freshness`, `last_error`, `replay_batch_id`, `source_checkpoint`, and `last_applied_event_id`.
2. Fix the handler, input ordering, schema mapping, or source replay window outside Cassie.
3. Resume replay from the last committed source checkpoint or rebuild the projection from the durable source event stream.
4. Run verification before trusting a rebuilt or repaired projection.
5. Swap or activate a materialized projection version only after replay is fresh, verification succeeds, and the active-version workflow reports a safe state.

Do not swap an active projection version after a failed replay solely because a batch retry completed locally. Verification and fresh checkpoint state are the operator guardrails.

## Evidence

- `tests/analytics.rs` module `projection_lifecycle` owns idempotent duplicate skip, out-of-order failed freshness, duplicate event ids inside one batch, restart hydration, atomic rollback of rows with replay metadata, no-op checkpoint advancement, and retry after an uncertain commit. Newly authored atomic replay assertions require execution on the qualified bundle.
- `src/midge/adapter/statement_read/tests.rs` authors a pinned Data-family snapshot regression for replay rows, scalar index entries, ledger, and stored checkpoint across a concurrent commit. `src/app/statement_read_tests/replay_native_unknown.rs` authors two narrow adapter-seam simulations of `OutcomeUnknown`: one after an actual successful native commit and one after dropping an uncommitted native transaction. They require publication fencing, restart and safe same-batch reconciliation; neither simulates an actual process crash or every native failure mode. The same module also authors a post-commit metadata-read failure and same-batch reconciliation probe. These tests have not been executed.
- `src/midge/adapter/statement_read/tests/replay_process_crash.rs` authors local
  child-process kill-and-reopen probes at a precommit boundary and after an
  acknowledged replay, including recovered row, scalar index, ledger,
  checkpoint and same-batch retry assertions. These probes are unexecuted; even
  if they pass, process death is not power-loss, buffered-WAL or cloud-durability
  evidence.
- `docs/production-readiness.md` records the Stable local projection lifecycle promotion and its retained native-disk replay, refresh, verification, and repair evidence. Deployment-profile alert thresholds remain open under #8 and are not implied by that promotion.
