# Projection Replay Contracts

Cassie replay is a local read-model ingestion contract. The source event stream, event ordering policy, retry policy, and projection handler code remain application-owned. Cassie owns local projection rows, replay checkpoint metadata, duplicate detection, freshness state, metrics, and catalog diagnostics.

This surface is Cassie-specific and part of the local projection lifecycle in
[Feature Support](feature-support.md). Atomic replay and its finite synchronous
recovery evidence were delivered under closed issue
[#1270](https://github.com/cntryl/cassie/issues/1270). The replayable policy is
implemented; the broader buffered/cloud loss and representative-workload
qualification remains tracked by [#1271](https://github.com/cntryl/cassie/issues/1271).
Deployment-level production readiness remains separate.

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

## Replayable durability policy (#1271, implemented with qualification limits)

The default replay API selects buffered local WAL.
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

The replay API policy is implemented in `src/app/replay.rs` and
`src/midge/adapter/storage_config.rs` (closed
[#1276](https://github.com/cntryl/cassie/issues/1276)); explicit SQL policy and its
remaining proof boundaries are recorded in
[Replayable SQL transactions](sql-replayable-transaction-contract.md). The
maintenance classification work is delivered under closed
[#1277](https://github.com/cntryl/cassie/issues/1277), while broader #1271
fault/loss qualification, operator diagnostics and representative benchmarks
remain separate obligations. The #1270 synchronous process-death evidence does
not qualify buffered or cloud recovery. The public checkpoint view's
statement-snapshot implementation and remaining limitations are described below
(closed [#1272](https://github.com/cntryl/cassie/issues/1272)).

## Deterministic Handler Expectations

- Events for one `(projection, source_identity)` pair must be replayed in monotonically increasing source position when positions are present.
- `event_id` must be non-empty, stable across retries, and unique for each source event within the projection/source pair.
- A new event must target a non-empty `document_id` for both puts and deletes. IDs are opaque strings, so a non-empty ID containing whitespace retains its exact identity. Cassie checks the durable event ledger before validating the document ID, so redelivery of an already committed event remains idempotent.
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
- empty event ids or empty document ids on new events
- duplicate new event ids inside one batch
- out-of-order event positions
- row, schema, constraint, index, or storage write errors
- replay-state persistence errors when the failure is known before commit

For preflight conflicts such as empty event IDs, empty document IDs on new events, out-of-order positions, and duplicate event IDs inside a batch, Cassie does not apply projection row writes, record new ledger entries, or advance `source_checkpoint` or `last_applied_event_id`. Previously acknowledged empty-ID rows retain their durable ledger entries and existing storage encoding. Rebuild an affected projection from retained source events with corrected IDs to recover those rows. [#1509](https://github.com/cntryl/cassie/issues/1509)

Replay publishes rows, transactional index mutations, event ledger entries, and checkpoint metadata in one transaction in the owning database family. A committed event's row mutations and checkpoint are visible together in the same storage snapshot. If the transaction aborts, neither its row mutations nor its ledger or checkpoint updates are visible. This includes a new event whose deletion targets a missing row: the ledger and checkpoint must still commit. Duplicate-only and empty batches still commit their replay metadata and diagnostics. A preparation based on a stale checkpoint or duplicate-ledger view fails with a retryable conflict; the caller retries the whole batch after reading current state. The in-process collection gate is not a substitute for storage transaction conflict detection. Cassie does not automatically retry an uncertain commit outcome; the caller reconciles the recovered ledger and retries the same deterministic batch.

Verification-manifest export prefers an exact canonical collection match. A bare or schema-qualified name can also select one committed projection by suffix when the exact match is absent. Ambiguous suffixes require a qualified name. Export reads the durable projection metadata, active output selection, generation, and emitted hash records in a single read-only snapshot of the owning database family. A missing or unreadable durable projection descriptor fails export. This keeps a replay checkpoint paired with the hash records from the same committed storage snapshot, including when a successful commit leaves the live projection catalog stale. Export preserves the verification state of stored hashes. [#1511](https://github.com/cntryl/cassie/issues/1511)

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
metadata is drawn from the same merged snapshot. This is implemented; see the
implementation status below for what is and is not yet proven.

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
selected design; its implemented form is described under implementation
status below.

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

**Selected fence design (2026-11-15, supersedes the candidate above).** Jev
(`target/jev/1272-fence-response.json`) selected a per-session, re-entrant
command-publication fence over stamp-and-retry (0.98). Source probes refined it:

- Readers acquire the fence at statement start, after the global
  `catalog_publication` read guard and before the descriptor snapshot and name
  binding, and release it after Data capture and before execution and portal
  delivery. Constant-only and other Data-free statements still take no Data
  snapshot.
- Every statement that stages storage and then separately publishes catalog
  state holds the same fence from staging through publication. This is a
  second acquisition site, because readers release theirs before execution.
  Every such mutator (CREATE/DROP/ALTER TABLE, indexes, views, sequences,
  materialized views, projection management) must be audited and covered; an
  uncovered mutator is a defect, not an accepted gap.
- The fence is re-entrant per thread so nested CTAS, CALL and embedded
  `execute_sql` on one session cannot self-deadlock.
- Lock order is global publication guard, then session fence, then the session
  transaction mutex, everywhere. The REPEATABLE READ capture and COMMIT paths
  already take the global guard first and must not wait for it while holding the
  fence. Acquisition honors cancellation and deadlines.
- Statements sharing one session serialize through bind and capture; independent
  sessions stay concurrent. Real pgwire connections already run one statement
  at a time.
- Jev's adversarial gap probability was 0.58 and lock-order risk 0.32; the
  findings above (mutator coverage and re-entrancy) are the confirmed leads.
  Retained REPEATABLE READ Bind owners keep their captured overlay and
  `rebind_statement_read` reuses it, so they do not mix revisions.

**Implementation status (#1272).** `pg_catalog.pg_projection_checkpoints` is
materialized from the statement's Data-family snapshot merged with the command's
own staged checkpoint-visible changes, with query-memory reservations retained
by the result and cancellation checks at the directory, scan and overlay
boundaries. A missing marker, a key and identity that disagree, an orphan
collection record or a duplicate directory entry fails closed with a repair or
rebuild diagnostic. The per-session re-entrant fence is acquired in
`execute_bound_catalog_scoped` after the global publication guard; catalog,
projection and retention statements hold it through execution and readers
release it after Data capture. Executable evidence: a synchronized
DDL-staged-before-publication regression (observed failing with
`CatalogObjectNotFound` before the fence), same-snapshot old-row regression,
staged CREATE/DROP with savepoints and rollback at READ COMMITTED and REPEATABLE
READ, post-commit metadata reread failure, nested CTAS over the view, missing
and mismatched markers, quoted identities, budget and cancellation.

Not yet covered by a dedicated test and therefore not claimed: authorization
and database isolation of this view specifically (existing database-scope
suites run in the gate but do not target it), materialized-projection rows,
restart on local storage, and retained REPEATABLE READ Bind portals.
Transaction-control statements (COMMIT, ROLLBACK, SAVEPOINT) do not take the
fence, so a concurrent same-session reader during ROLLBACK TO SAVEPOINT is not
protected against observing the storage restoration before the catalog
restoration. Fencing them was tried and rejected: the supported schedule in
which a later ROLLBACK TO recovers a session while an older one is still
unwinding (`should_preserve_later_recovery_after_old_savepoint_restoration_error`)
requires savepoint control to stay concurrent on one session. Rollback ordering
belongs to #1290's retirement work. Existing integration suites exercise this
view across database scope, snapshot restore and materialized projection
lifecycle; they run in the gate but were not written for this issue. Autocommit CREATE TABLE publishes under the global write guard
without a separate staged overlay and is unaffected.

Preflight and known precommit row-write failures attempt to persist failed freshness, guarded by the metadata version read during preparation. A changed checkpoint returns conflict rather than overwriting newer metadata. A storage stall preserves its original retryable error if the separate diagnostic write also fails. Failed metadata is hydrated on restart when that diagnostic commit succeeds; durable failure diagnostics cannot be promised when storage is unavailable. A fenced writer must not submit another diagnostic write. A failure returned after commit may have an uncertain outcome; inspect recovered checkpoint and ledger state and retry the same batch instead of assuming either commit result. In particular, a failed metadata reread after a successful commit can return an error while the rows, ledger and checkpoint are already committed and the live catalog remains stale. A same-batch retry reconciles duplicate events when storage is available. The caller owns its external source acknowledgment. An external offset can survive loss of the entire Cassie database: recovery must rewind or rebuild from retained source events rather than trusting an offset beyond recovered Cassie state. The implemented replayable policy above does not by itself establish buffered power-loss or cloud-cache-loss qualification; #1271 retains those independent evidence requirements.

## Cancellable Replay

`Cassie::replay_projection_batch_cancellable` and
`Cassie::replay_projection_batch_strict_cancellable` take an explicit
`QueryCancellationHandle` ([#1274](https://github.com/cntryl/cassie/issues/1274)).
The existing `replay_projection_batch` and `replay_projection_batch_strict`
entrypoints are unchanged and take no handle.

- **Authority.** The explicit handle is added to any ambient request
  cancellation or deadline on the calling stack; neither replaces the other.
- **Check points.** Cancellation is checked before and while waiting for the
  referential and collection write gates, for each event during preparation,
  after preparation, and while waiting for and immediately after acquiring the
  database publication gate. That last check, taken under the publication gate
  directly before the native Midge commit is invoked, is the commit-submission
  boundary.
- **Before submission.** A cancellation observed at or before that boundary
  returns `QueryCanceled` (or `DeadlineExceeded` for an ambient deadline). Rows,
  derived indexes, the event ledger and the checkpoint are unchanged, no gate is
  retained, and the projection is not marked failed: no failure diagnostic is
  written with the interrupted authority. The same batch can be retried and
  applies every event.
- **After submission.** A confirmed commit returns success. Its metadata reread
  and catalog registration run with the request authority retired, so a late
  cancellation cannot turn a committed batch into an error. Post-commit derived
  maintenance keeps the request authority as before: if it is interrupted its
  durable debt remains and is repaired later, without affecting the reported
  outcome. An unknown native outcome returns the storage error, latches
  recovery-required fencing and must be reconciled after recovery by retrying
  the same batch; it is never reported as a cancellation.
- **Existing entrypoints under an ambient control.** `replay_projection_batch`
  and `replay_projection_batch_strict` take no handle and behave as before when
  the calling stack has no request control. When one is present (a REST or SQL
  request deadline or cancellation), they now observe it at the same check
  points and return the interruption without writing a failed-projection
  diagnostic; previously the interruption surfaced only at the publication gate
  and was followed by an attempted diagnostic write.
- **Midge boundary.** Published `cntryl-midge` 0.5.2 offers no cancellation of an
  admitted commit, so a cancellation that arrives during native submission
  cannot abort it. This is a property of the published API, not a blocker for
  the contract above.

## Operator Workflow After Replay Failure

1. Inspect `pg_catalog.pg_projection_checkpoints` for `freshness`, `last_error`, `replay_batch_id`, `source_checkpoint`, and `last_applied_event_id`.
2. Fix the handler, input ordering, schema mapping, or source replay window outside Cassie.
3. Resume replay from the last committed source checkpoint or rebuild the projection from the durable source event stream.
4. Run verification before trusting a rebuilt or repaired projection.
5. Swap or activate a materialized projection version only after replay is fresh, verification succeeds, and the active-version workflow reports a safe state.

Do not swap an active projection version after a failed replay solely because a batch retry completed locally. Verification and fresh checkpoint state are the operator guardrails.

## Evidence

The current default-suite evidence below is from
`843500f857926f3f1986942ae634912eb692bfd2`: 14,460 passed, zero failed and nine
inherited ignores. The [runtime qualification record](evidence/runtime-closeout-2026-11-17/README.md)
separates that executed source, subsequent mechanical/name-only changes and
proofs, affected gates, benchmark source, and final publication-head hosted checks.

- `tests/analytics.rs` module `projection_lifecycle` covers idempotent duplicate
  skip, out-of-order failed freshness, duplicate event ids, restart hydration,
  atomic rows/replay metadata, no-op checkpoints and uncertain-commit retry.
  These passed at `843500f8`; the earlier `525e9a96` full-suite record remains
  historical evidence rather than the identity of the current run.
- `src/midge/adapter/statement_read/tests.rs` covers a pinned Data-family snapshot
  for rows, scalar indexes, ledger and checkpoint across a concurrent commit.
  The local-storage counterpart is
  `src/midge/adapter/statement_read/tests/replay_durable_snapshot.rs`.
  `src/app/statement_read_tests/replay_native_unknown.rs` covers narrow adapter
  unknown-outcome seams before/after native commit and a post-commit metadata-read
  failure. These cases passed in the default suite at `843500f8`; they do not
  simulate every native failure mode.
- `src/midge/adapter/statement_read/tests/replay_process_crash.rs` passed all five
  controls at `843500f8`: one writer-lease retry control and four strict/buffered
  local process-kill/reopen cases before commit or after acknowledgement. They
  verify recovered rows, scalar index, ledger, checkpoint and same-batch retry.
  Process death does not establish power-loss or cloud-cache-loss recovery.
- All 34 selected feature-enabled controls passed at `843500f8`, including replay
  API, SQL, pre-native sequence admission, actual wire COPY Data policy and typed
  adapter-conflict retry. Simulated background/strict cloud policy observations
  do not qualify provider acknowledgement, native faults or cloud-cache loss.
- The [48-control replay record](evidence/replay120-2026-11-16/README.md) remains
  historical source `58e6c96e`. Fresh eight-cell embedded-SQL diagnostics use
  `168527ec5804b47511a3a662484984a935be5c11`, test profile, 64 MiB and a 120-second
  per-query deadline. They cover default/replayable, both shapes and both batch
  sizes with one fresh store per cell; they do not establish pgwire latency or
  across-host variance. Their complete source, samples and counter accounting
  are linked from the runtime qualification record.
- Foreground maintenance remains part of acknowledgement: column/projection-hash
  maintenance, time-series rebuild and debt cleanup can retain strict barriers.
  Engine-wide WAL/ledger counters include background work and omit unaccounted
  filesystem syncs. `wal_accounted_syncs` already includes successful WAL fsync
  and ledger syncs; the fields are not added together. No universal physical-sync
  reduction follows from selecting buffered primary policy. Broader qualification
  remains tracked by [#1271](https://github.com/cntryl/cassie/issues/1271).
- `docs/production-readiness.md` retains the lifecycle, refresh, verification and
  repair contract. Deployment-profile alert thresholds are separate from this
  replay evidence.

The later [finite simulated-cache-loss and read record](evidence/replay-read-recovery-2026-11-17/README.md)
adds nine cloud controls with observed strict position 2/replayable position 1
for API and SQL, followed by reconciliation, at `40a08ce2`. Its separately
qualified closed-store reader at `a5fd9176` passed eight profiles / 408 actual
timed queries with named index or scan attestations and original-byte checks.
Those sources and fresh-versus-historical input identities remain explicit; the
earlier B process/policy results above are not relabelled as cloud-cache-loss
measurements. Physical power-loss, real provider semantics and native-wide
quiescence remain open under #1271 and #1188.
