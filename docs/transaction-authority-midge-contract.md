# Transaction authority within Midge as published

Status: **the finite staged-effect, sealing, operation-owner, shared-setting and
retained-worker lifetime repairs are implemented.** The full default suite and
34 feature controls passed at `843500f8`; later mechanical lint and test-name
changes have separate equivalence and affected-gate evidence in the [qualification record](evidence/runtime-closeout-2026-11-17/README.md).
Final publication-head hosted qualification is recorded there separately. A
per-transaction cell refactor remains deferred; it is not an acceptance substitute.
This document records the transaction-authority work tracked by
[#1290](https://github.com/cntryl/cassie/issues/1290),
[#1302](https://github.com/cntryl/cassie/issues/1302) and
[#1279](https://github.com/cntryl/cassie/issues/1279), so that Cassie
works with published `cntryl-midge` 0.5.2 exactly as it is. It supersedes the
admission-permit-and-drain and admission-lease-with-compensator directions
recorded on those issues. Implementation boundaries are recorded in each section below.

## What Midge provides, and what it does not

From `docs/transactions-and-mvcc.md` and
`docs/user-guides/transaction-durability-contract.md` in the published crate:

| Midge behaviour | Consequence for Cassie |
| --- | --- |
| A transaction is scoped to one column family; there is no cross-family atomic transaction. | One atomic unit is one native transaction in the owning database's Data family. |
| `commit()` publishes one transaction all-or-nothing, including across a crash. | This is the only atomicity Cassie may rely on. |
| A caller timeout does not cancel admitted storage work, and no API cancels an admitted commit. | Cancellation can only win before native submission. |
| `commit()` can return `OutcomeUnknown` while the write still applies; Midge never retries. | Never retry, never assume an abort, fence until recovery. |
| A transaction pins its snapshot until it is dropped. | No design may hold a native transaction across unbounded work. |
| `assert_value` guards only the named keys, from the transaction's start snapshot. | Usable as a single-key guard at COMMIT; not a predicate lock. |
| `Midge::data_tx` and `database_tx` hand out raw writable transactions. | Cassie's gates cannot fence them. |

The earlier directions asked for things this table rules out: waiting for or
bounding admitted native work, owning an independently committed effect until a
compensator is registered, and fencing raw native writers.

## Principle

Every transactional effect is staged in Cassie and published by the one atomic
Data-family commit at COMMIT. Cassie does not commit a transactional effect
separately and compensate for it later, and it does not wait on or try to
cancel admitted Midge work. Ownership is established by construction, not by
locks held across native I/O.

## Selected design

Jev's choice probabilities are in parentheses; the request and response are in
ignored `target/jev/midge-as-is-design-{request,response}.json`.

### 1. Identity restart is staged, not committed (#1302) (1.00)

A transactional `TRUNCATE ... RESTART IDENTITY` stages the restarted cursor in
the transaction's `StorageOverlay`, using the mechanism that already serves a
sequence created inside the transaction ("Private"). Sequences and rows live in
the same Data family, so the restart publishes in the same native transaction
as the truncation. Savepoint rollback restores the staged cursor, ROLLBACK
discards it, and there is no compensator and no session-shared restart list.
This removes all three observed #1302 hazards: a compensator attached to a
later transaction, a rollback rewinding another session's advance, and the
process-death window in which the restart survives but the truncation does not.

At COMMIT the native transaction asserts the sequence key against the cursor
observed when the restart was staged. If another session advanced the sequence
in between, the commit fails and nothing is published, instead of silently
discarding the values that session allocated.

**Implemented for #1302.** `TRUNCATE ... RESTART IDENTITY` inside a
transaction stages the restarted cursor in the storage overlay, marks the
sequence so later allocations in that transaction read the staged cursor, and
begins the transaction's catalog overlay so COMMIT publishes the storage
overlay with the rows. Other sessions keep allocating from the published
cursor until COMMIT. The session-shared restart list, its rollback restorer
and the commit-time discard were removed; nothing is committed natively and
nothing is compensated. Outside a transaction the restart is still immediate.

A transaction that restarts an identity is treated as carrying catalog state,
so its COMMIT takes the global catalog publication guard and a declared
replayable transaction is promoted to strict.

A concurrent advance by another session between the restart and COMMIT fails
the COMMIT through Midge's value assertion on the sequence key: the overlay
records the value each key had when the transaction first changed it. Nothing
is published, the transaction ends, and the other session's allocations are
kept. A test covers this.

Still open: that failure currently surfaces as the generic retryable storage
conflict (`57P03`), and the SQLSTATE a client should see is not selected. An explicit `setval` by another session that restores
identical cursor bytes cannot be seen by a value assertion (Jev rated the
assertion sufficient at 0.75, not higher). The process-death window is removed
by construction, since there is no separate native commit, but no
process-kill test was added.

`nextval` and `setval` on an existing sequence outside this path stay
independent and are never rolled back, matching PostgreSQL. They are recorded
on the owning transaction before they run and only raise strictness (#1279).

### 2. One cell per transaction (#1290) (1.00)

In-place mutation of one shared session struct is replaced by one cell per
transaction. The cell owns that transaction's writes, storage overlay, catalog
overlay, `SET LOCAL` restoration, unique-key owner, savepoints, portal marks,
projection write authority and status. BEGIN installs a new cell. COMMIT and
ROLLBACK retire their own cell and clean only resources that cell owns.

Every operation captures the cell at admission. It can mark, publish to or
clean only that cell. A late operation acts on a retired cell and its effect is
discarded, so it cannot fail, reset or clean a newer transaction. Nothing waits
and no lock is held across native I/O. This generalises the operation-scoped
failure authority already merged in #1296 and #1299 and the Arc-owned
projection write authority merged in #1305.

**First increment implemented: atomic retirement.** COMMIT and ROLLBACK now
complete all cleanup of what the transaction owned (schema context, unique-key
owner, cursors and `SET LOCAL` or session-state restoration) before releasing
the session transaction mutex, and BEGIN snapshots session settings under that
mutex. A later transaction cannot start, attach a schema context, declare a
cursor or capture the previous transaction's `SET LOCAL` values until the
previous one is fully retired. The cleanup is in-memory; no storage work runs
under the mutex. `object_lookup_context` was reordered so the lock order is
always transaction mutex, then schema context. Jev rated landing this as an
increment at 0.96 and a remaining hazard at 0.16. It does not replace the cell:
an operation that outlives its transaction is still stopped by generation,
epoch and failure-authority checks rather than by ownership of a cell.

**Second increment implemented: COMMIT owns its transaction.** While a COMMIT
is sealed, another `BEGIN`, `COMMIT`, `ROLLBACK` or savepoint statement on the
same session is refused immediately with `25000` ("a COMMIT is in progress on
this session; retry after it completes"). Cassie does not wait, because Midge
cannot cancel an admitted commit. Only the thread running that COMMIT can mark
the sealed transaction failed, so a refused statement does not change the
outcome the COMMIT reports. COMMIT's final cleanup retires only the transaction
it sealed: if another path already ended it, or a later transaction has begun,
nothing is reset. The committing thread is never refused its own transaction
control.

**Third increment implemented: direct rollbacks and wire status.**
`rollback_transaction` is a no-op on any thread other than the one running a
sealed COMMIT, so the rollbacks pgwire error handling, CTAS and modifying CTEs
issue directly cannot end a transaction whose native commit is in flight; the
committing thread retires it once the outcome is known. ReadyForQuery reports
`E` to any other thread while a COMMIT owns the transaction, so a client never
sees a usable `T` or `I` for a transaction that is not. A failure mark from
another thread does not change the sealed transaction while the COMMIT runs. It
is remembered instead: if the COMMIT then fails before submission and releases
its seal with the transaction still open, the transaction becomes failed; if
the COMMIT retires the transaction, the remembered mark is discarded and never
reaches a later transaction. Portal cleanup runs only when the status is idle, so a
refused rollback also leaves portals in place until the COMMIT retires.

Cancellation while COMMIT waits does not arise: COMMIT never waits. It seals,
submits and finishes, and cancellation is honoured only before submission.

Jev's adversarial probability that a hazard remains through state that cannot
live in the cell is 0.59, so this is the main proof obligation.

**Session state inventory (source-checked, not re-judged by Jev).** Every field
of `CassieSession` has one of three rules. None of them needs a cell to be safe
today; the table records why, so a later change cannot silently break it.

| Field | Rule | Guard |
| --- | --- | --- |
| `transaction` | Owned by the transaction. | Reset and cleaned under its own mutex at COMMIT and ROLLBACK; operations use generation, rollback epoch, failure authority and the COMMIT seal. |
| `schema_context` | Root context belongs to the transaction; each statement fork owns its context wrapper and cloned default-database bytes. | Root attachment validates captured ownership under root→context order; forks retain their acquired catalog/descriptor references with a last-clone memory reservation. |
| `session_objects` (cursors, prepared statements) | Transactional lifetimes and existing named/with-hold session lifetimes are preserved. | Shared mutations validate the actual root authority before root→objects writes, including detached callers; own-retirement publication is restricted to the exact lifecycle-root receipt. |
| `transaction_unique_waits` | Owned by the transaction. | Keyed by the transaction's unique-owner id, released at retirement. |
| `transaction_clock` | Owned by the actual transaction root. | Validate failure authority (UUID, rollback epoch and generation) under root→clock order through shared clock read/publication; forks use their actual-root lineage. Idle uses statement start without touching the shared clock. Only the exact lifecycle-root own-retirement receipt permits that read-only Idle view; forks and ABA/unrelated retirement do not. See the [clock contract](transaction-owner-mutation-inventory.md#transaction-clock). |
| `search_path`, `settings` | Root clones share session payload and restoration history; detached forks own private copies of both. | BEGIN snapshots and terminal restoration run under the root mutex. Scoped leaves validate the actual root before changing the appropriate payload/history together. A fork's LOCAL restoration history never restores over an independent root SET; projection mode retains its actual-root transaction freeze. |
| `sequence_values` (`currval`, `lastval`) | Session-owned; survives ROLLBACK. | Reset and successful-native history publication use sequence-values→actual-root order. A stale result cannot overwrite a replacement's history; the already applied independent native effect and global catalog cursor remain published. |
| `random` | Session-owned. | No transaction meaning. |
| `procedure_calls` | Owned by the statement fork. | Copied at fork; a recursion guard, not transaction state. |

An operation that outlives its transaction remains an executable proof obligation.
The current bundle carries one deferred owner through simple, extended and COPY
workers, converters and outer error/terminal wrappers. Query-slot rejection before
capture has no mutation authority; established protocol-origin ownership survives
later saturation. Own BEGIN/savepoint transitions update only that operation's
carrier. Root mutation, mutable-alias export and terminal cleanup validate UUID,
rollback epoch and generation under their actual root mutex.

Wire portals carry immutable lifetime tokens. Bind and final Parse/Close
publication validate the captured operation; fresh Execute checks expected portal
lifetime before own stage creation and after worker admission. An old portal never
captures failure authority over B. Exact own idle-to-active receipts adopt surviving
preparation portals for implicit or explicit BEGIN. Reconciliation reads actual
current lifetime nonblockingly and filters owners, ignoring epoch for portal
lifetime while checking it for operation admission. Named statement lifetimes and
unnamed Query destruction remain unchanged.

These source repairs do not close #1290 or #1279 until the focused synchronized
probes and complete combined gate run against the final integrated source.

### 3. COMMIT seals; it does not drain (#1279) (1.00)

COMMIT seals its cell under the session mutex, freezing the declared projection
write mode and the captured writes in one step, then submits. A fork that
publishes afterwards is rejected by cell identity. Because transactional
effects are staged, nothing a late fork does is durable, so there is nothing to
drain. This removes the self-wait, lock-order and async-reactor-stall hazards
that #1279 lists for a draining COMMIT.

**Implemented for #1279.** COMMIT seals the transaction before capturing its
writes; a later fork publication and a later mode declaration are rejected, and
a failed COMMIT that leaves the transaction open releases the seal.
Transactional `COPY ... FROM` no longer promotes a declared transaction.

Replayable transactional COPY becomes a consequence of this: COPY rows are
staged like any other rows, so a declared transaction that stages only rows and
its checkpoint can keep the replayable policy once COPY's fork is bound to the
cell. 

### 4. Outcomes and cancellation

- **Cancellation** is honoured up to the final check before native submission
  and not after (0.91). This is the same boundary cancellable replay uses.
- **`OutcomeUnknown`.** Observed and now pinned by tests: a SQL `COMMIT` whose
  native outcome is unknown returns `58030` and ends the session's transaction
  (neither active nor failed). The storage recovery-required fence is latched
  for the whole instance: every later read, write and `BEGIN`, on that session
  and on others, returns `58030` without being submitted, and `ROLLBACK` is
  accepted. Nothing is retried. After the store is reopened the transaction is
  present in full if the commit had applied and absent if it had not, and
  writes are accepted again. This matches the fence-until-recovery option (Jev
  0.67); no code change was needed. Operator guidance is in
  [local diagnostics](local-diagnostics-contract.md): restart, then reconcile
  through the transaction's own checkpoint.

### 5. Raw native writers (0.99)

Cassie-internal keys (sequences, maintenance debt, collection generations and
projection metadata) are documented as reserved. A raw `data_tx` writer that
modifies them is outside the supported interop contract. Cassie adds a
single-key `assert_value` where that is sufficient and does not attempt a
registry that fences previously issued native transactions.

## Order of work

1. Staged identity restart (#1302). It is the smallest change, removes an
   observed defect and deletes the session-shared restart list the cell would
   otherwise have to own.
2. Per-transaction cell with owner-scoped cleanup (#1290).
3. Sealing COMMIT, policy freeze and replayable COPY (#1279).

Jev preferred this order at 0.76 over starting with the cell (0.23).

## Not provided by Midge and not worked around

- Atomicity across databases or families. A transaction that touches more than
  one database stays rejected.
- Cancelling or bounding an admitted commit.
- Power-loss ordering beyond the published durability frontiers.

Jev rated the chance that this design still needs a capability Midge does not
publish at 0.19. That is a lead to check during implementation, not a proof
that none is needed.
