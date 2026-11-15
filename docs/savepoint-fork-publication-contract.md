# Savepoint rollback and detached fork publication

Status: #1286 / #1279 prerequisite. The integrated source was reviewed and the
complete locked suite passed on `d76c9258bb3bdc862dbc0e8068a4447277b25cae`.
This is not the transaction admission, replayable SQL mode, or full
rollback-isolation contract.

A detached statement fork records an in-memory rollback epoch from its parent
while holding the same transaction mutex that protects the parent snapshot. A
successful `ROLLBACK TO` advances that epoch, even if the restored row and
catalog contents happen to equal the fork's original before-images. Under the
same parent mutex, publication checks the captured epoch before merging any
row, catalog, storage or conflict-intent state. A fork captured before that
rollback is rejected with the existing session-changed error; a new fork taken
afterward can publish normally. Concurrent forks before a rollback continue
using existing before-image/metadata conflict handling. No session mutex is
held across statement execution, no new global lock is added, and this slice
does not change WAL, cloud or write policy.

The epoch is a checked in-memory counter for the current transaction, separate
from every savepoint payload. It is never restored from a savepoint or silently
wrapped; exhaustion must return an error before cursor pruning or restoration.
A true Idle-to-active transaction start resets it to zero under the same mutex,
so a full rollback followed by BEGIN recovers from epoch exhaustion. Promoting
an implicit transaction to explicit preserves its existing epoch. The existing
generation check rejects a batch from the immediately previous transaction;
that generation currently uses wrapping arithmetic, so this increment does not
claim absolute identity protection after generation wrap. Missing
savepoints and ordinary fallible pre-restore cursor-pruning errors must not
invalidate otherwise valid forks. After target validation and checked addition,
advance the epoch under the parent mutex before pruning. The source's pruning
routine performs its ordinary fallible checks before its `retain` mutation;
restore the prior epoch under that same mutex on those ordinary errors. If
pruning instead unwinds after mutation, the advanced epoch remains, preventing
a prior fork from publishing. Once restoration has begun, the epoch also remains
advanced even if subsequent unique-owner restoration fails outside the mutex:
the previous fork must not publish into partially restored state. This does not
claim broader panic recovery or atomic restoration of every session component.

Invalidation is deliberately conservative: every in-flight older fork is
rejected after rollback-to, including an independent fork begun before an
internal CTAS savepoint. This avoids silently publishing work captured against
an earlier transaction epoch. Nested/CTAS success and failure controls passed
in the full suite on the head identified above.

Source witness at merged main `e91e92433e5d6c185166b471a06f0690d5c695d4`:
`session/statement_batch.rs` captures generation and before-images, then checks
them at publication; `session/savepoints.rs` restores a savepoint without
changing the transaction generation. `BEGIN; SAVEPOINT s;` followed by a fork
that stages a previously absent row, `ROLLBACK TO s`, and late fork publication
can pass the existing equal-before-image and generation checks. The regression
must construct this order deterministically, assert no staged row survives,
and prove a fresh post-rollback fork succeeds. Both passed in the full suite
on the head identified above. No baseline test has run.

This epoch only fences **publication into the parent session**. It cannot undo
or prevent an independently durable `nextval`, a COPY predicate effect, a native
write already submitted, or a later concurrent COMMIT. A savepoint rollback's
post-unlock cleanup still needs its own completion/admission proof. Those
requirements, transferable execution ownership, cancellation and recovery
fencing remain open under #1279 and are not qualified by this increment.

Jev selected the epoch prerequisite 1.0 and identified the source schedule at
0.90, with failed-rollback ordering risk 0.80; receipts live in the preserved
`cassie-sql-copy-admission-1279/target/1279-design/savepoint-fork-{request,response}.json`.
The full suite also passed missing-savepoint, canceled preflight, exhaustion,
new-transaction recovery and later owner-key failure controls. The unwind
probe injects after successful pruning, not inside pruning. Jev is advisory;
source review and executable checks do not establish broader admission.
