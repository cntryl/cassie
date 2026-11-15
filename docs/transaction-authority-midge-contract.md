# Transaction authority within Midge as published

Status: **selected design; sections 1 and 3 and the first increment of section 2 implemented; the per-transaction cell, outcome handling and reserved-key documentation are not**. This document selects how the
remaining transaction-authority work
([#1290](https://github.com/cntryl/cassie/issues/1290),
[#1302](https://github.com/cntryl/cassie/issues/1302) and the open part of
[#1279](https://github.com/cntryl/cassie/issues/1279)) is built so that Cassie
works with published `cntryl-midge` 0.5.2 exactly as it is. It supersedes the
admission-permit-and-drain and admission-lease-with-compensator directions
recorded on those issues. Only section 1 is implemented.

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

Jev's adversarial probability that a hazard remains through state that cannot
live in the cell is 0.59, so this is the main proof obligation. Session-level
state that outlives a transaction (session `SET` values, the search path, the
`currval` cache, prepared statements and holdable cursors) must be inventoried
field by field, and each one given a rule: owned by the cell, or changed only
through an operation that is checked against the current cell.

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
- **`OutcomeUnknown`** retires the cell as outcome-unknown, latches the existing
  storage recovery-required fence and reports the error without retrying
  (0.67, with 0.32 on "none"). The client-visible status and what a following
  BEGIN returns while fenced are not settled and need a decision before
  implementation.

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
