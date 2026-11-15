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

## Replayable durability policy (#1271, selected; implementation pending)

The default replay API changes from synchronous local writes to buffered WAL.
Existing callers that require the previous local acknowledgement should migrate
to `replay_projection_batch_strict`; on cloud this explicitly waits for the
strict cloud policy instead of inheriting background upload.

A replayable write is a declaration that the projection can be reconstructed
from its retained source events. The existing `replay_projection_batch` API selects this profile by default;
`replay_projection_batch_strict` selects strict replay for callers that require
a durable acknowledgement. Declared SQL projector work
uses `BEGIN; SET LOCAL cassie.projection_write_mode = 'replayable';` before any
writes, followed by row and checkpoint writes in the same transaction. The
setting is transaction-local, cannot be installed through session/startup SET,
and must not survive COMMIT, ROLLBACK, a failed transaction, or connection reuse.
Undeclared SQL, autocommit and COPY retain their existing policy. Mixed writes
must use the strictest applicable policy, including schema, roles, sequences,
security metadata and other effects that are not themselves replayable; a write
cannot be weakened solely because its table looks like a projection. Unknown
write classifications fail closed. Savepoint rollback restores the declared
setting and staged changes, while independently published sequence effects must
retain their required durability even if a savepoint is rolled back.

Local replayable commits use Midge `WriteOptions::buffered()` and keep the WAL
enabled. Their acknowledgement means the transaction is accepted locally, not
that a power loss cannot discard it. For cloud background mode, replayable
commits use Midge `cloud_async()`: visibility and local acknowledgement can
precede remote upload. A configured cloud-strict policy is never downgraded;
the explicit strict replay option uses local `sync()` or cloud `cloud_strict()`.
Midge `best_effort()` is not a replayable policy. Postcommit index and derived
maintenance may perform additional writes or syncs before acknowledgement; the
primary commit mode alone does not establish a barrier-free path.

Rows, transactional indexes, duplicate ledger and checkpoint share one commit,
but that atomicity does not make a buffered acknowledgement durable or make an
external source offset safe. After a restart or loss of local/cloud cache, the
projector must reconcile against the recovered Cassie checkpoint, rewind the
source to that checkpoint or rebuild from an earlier retained point, then replay
and verify before serving the projection as ready. An external offset that can
outlive Cassie must not be the only recovery authority; source retention must
cover the rewind or full rebuild. If the source cannot be rewound far enough,
the projection remains unavailable until rebuilt from another authoritative
source. Cassie does not currently automate this source coordination. No fixed
loss window, power-loss guarantee, cloud cache-loss guarantee or throughput
improvement is claimed without independent qualification.

This section selects the intended contract; #1271 implementation, fault/restart
qualification, operator diagnostics and representative benchmarks are pending.
The #1270 synchronous process-death evidence does not qualify buffered or cloud
recovery. The public checkpoint view's separate snapshot limitation remains
tracked by #1272.

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

The selected checkpoint-view contract reads projection metadata from the same
statement Data-family snapshot as projection rows, merged with the same
statement's admitted immutable relevant storage overlay. Capture the overlay
and row view with a proven command-publication fence so own staged CREATE/DROP,
savepoints and rollback cannot mix revisions. The session transaction mutex
alone is insufficient: DDL can stage its storage overlay before separately
publishing its catalog update. REPEATABLE READ retains its base
Data snapshot but captures own storage changes per command. Preserve exact
case-sensitive canonical identities and database isolation. Modern collection
creation writes an initial projection metadata record in the collection's Data
transaction. A collection present in the captured collection directory but
missing its metadata marker is indistinguishable from lost state and fails
closed with an actionable repair/rebuild diagnostic; it must not receive a
fabricated default or a later live-catalog checkpoint. Pre-release older stores
with missing markers need an explicit repair/rebuild before this view can be
used. The directory, metadata scans, overlay capture and materialized output
must check cancellation, reserve memory before allocation, retain reservations
with results, and release them on failure/drop. Materialized projection
metadata is drawn from the same merged snapshot. This is a selected target;
the command-publication fence, implementation and validation remain pending.

The selected fence is per session and protects DDL storage staging together with
its corresponding catalog publication, and protects capture of each matching
catalog/Data owner. It is released before post-capture hooks, query execution,
and retained portal delivery. Independent sessions and same-session row-only
operations remain concurrent. When both locks are needed, acquire the global
catalog publication guard before the session fence; never wait for that global
guard while holding the session fence. Guard acquisition must honor cancellation
and deadlines. A COMMIT first classifies its required global guard without
holding the session fence, then takes global before session and rechecks the
transaction generation and required guard strength. If either changed it drops
both and retries or fails closed; it must reuse an already-held implicit-DDL
publication guard. Bind descriptor validation and its retained catalog/Data
owner must observe one protected revision. No overlay mutex may be retained while
opening a scoped Midge transaction, which acquires that mutex internally. The
relative order of the database publication gate and session fence, nested SQL,
and reentrancy must be proven before implementation; this paragraph records the
selected target, not an implemented synchronization guarantee.

A candidate for reconciling this fence with conditional Data capture is to
capture immutable catalog/descriptor authorities and a session publication
stamp under the short fence, plan and authorize without the fence, then reacquire
it before Data capture and verify that the stamp still matches. A mismatch may
retry only pure, not-yet-executed planning under the original query controls;
CTAS, CALL and other operations with side effects cannot be restarted this way.
Retained REPEATABLE READ Bind owners must retain their original authority. This
candidate must preserve the existing rule that constant and other proven
Data-free statements do not acquire a Data snapshot. Transaction generation is
not a sufficient stamp: it does not change on every staged DDL mutation. All
relevant partial failures, storage and catalog changes, savepoint restoration,
transaction controls and COPY publication need comprehensive coverage, and
mutable catalog aliases must not escape as supposedly immutable authorities.
Mutation coverage, lock ordering, budget ownership, retry behavior and tests
remain design and implementation obligations, not established guarantees.

Preflight and known precommit row-write failures attempt to persist failed freshness, guarded by the metadata version read during preparation. A changed checkpoint returns conflict rather than overwriting newer metadata. A storage stall preserves its original retryable error if the separate diagnostic write also fails. Failed metadata is hydrated on restart when that diagnostic commit succeeds; durable failure diagnostics cannot be promised when storage is unavailable. A fenced writer must not submit another diagnostic write. A failure returned after commit may have an uncertain outcome; inspect recovered checkpoint and ledger state and retry the same batch instead of assuming either commit result. In particular, a failed metadata reread after a successful commit can return an error while the rows, ledger and checkpoint are already committed and the live catalog remains stale. A same-batch retry reconciles duplicate events when storage is available. The caller owns its external source acknowledgment. An external offset can survive loss of the entire Cassie database: recovery must rewind or rebuild from retained source events rather than trusting an offset beyond recovered Cassie state. The selected #1271 policy above remains unimplemented and does not establish crash atomicity for a new durability policy.

## Operator Workflow After Replay Failure

1. Inspect `pg_catalog.pg_projection_checkpoints` for `freshness`, `last_error`, `replay_batch_id`, `source_checkpoint`, and `last_applied_event_id`.
2. Fix the handler, input ordering, schema mapping, or source replay window outside Cassie.
3. Resume replay from the last committed source checkpoint or rebuild the projection from the durable source event stream.
4. Run verification before trusting a rebuilt or repaired projection.
5. Swap or activate a materialized projection version only after replay is fresh, verification succeeds, and the active-version workflow reports a safe state.

Do not swap an active projection version after a failed replay solely because a batch retry completed locally. Verification and fresh checkpoint state are the operator guardrails.

## Evidence

- `tests/analytics.rs` module `projection_lifecycle` covers idempotent duplicate skip, out-of-order failed freshness, duplicate event ids inside one batch, restart hydration, atomic rollback of rows with replay metadata, no-op checkpoint advancement, and retry after an uncertain commit. The full locked suite passed on `525e9a96c80dc2ad31e39e0665efbdcc6a44fbae`.
- `src/midge/adapter/statement_read/tests.rs` covers a pinned Data-family snapshot for replay rows, scalar index entries, ledger, and stored checkpoint across a concurrent commit. The local-storage counterpart in `tests/replay_durable_snapshot.rs` also checks restart and retry. `src/app/statement_read_tests/replay_native_unknown.rs` covers two narrow adapter-seam simulations of `OutcomeUnknown`, before and after native commit, plus a post-commit metadata-read failure. These tests passed on the same head. They do not simulate every native failure mode.
- `src/midge/adapter/statement_read/tests/replay_process_crash.rs` covers strict and buffered local child-process kill-and-reopen at a precommit boundary and after acknowledgement, including recovered row, scalar index, ledger, checkpoint and same-batch retry assertions. These cases passed on the same head. Process death does not establish power-loss or cloud-cache-loss recovery.
- Feature-gated replay policy tests passed on the same head and observe the first native commit policy and per-policy attempts, including simulated background and strict cloud configuration. Simulated provider policy selection does not qualify real provider acknowledgement or cloud-cache loss recovery.
- The local strict-versus-buffered diagnostic benchmark on the same head observed additional strict foreground commit attempts in every buffered run. Replay's selected commit is followed before return by column-batch and projection-hash maintenance and time-series index rebuilds; debt cleanup uses strict write options. Engine-wide WAL and ledger counters may include background work and omit some filesystem syncs, so they cannot attribute every physical sync to replay. Broader workload measurements and maintenance-barrier work remain under [#1271](https://github.com/cntryl/cassie/issues/1271).
- `docs/production-readiness.md` records the Stable local projection lifecycle promotion and its retained native-disk replay, refresh, verification, and repair evidence. Deployment-profile alert thresholds remain open under #8 and are not implied by that promotion.
