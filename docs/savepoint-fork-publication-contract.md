# Savepoint rollback and detached fork publication

Status: selected #1286 / #1279 prerequisite; tests and implementation require
review and executable qualification. This is not the transaction admission,
replayable SQL mode, or full rollback-isolation contract.

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

The epoch is a checked monotonic in-memory counter, separate from every
savepoint payload. It is never restored from a savepoint or silently wrapped;
exhaustion must return an error before cursor pruning or restoration. Missing
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
an earlier transaction epoch. Test nested/CTAS paths and confirm their existing
success, error and new-fork behavior before qualification.

Source witness at merged main `e91e92433e5d6c185166b471a06f0690d5c695d4`:
`session/statement_batch.rs` captures generation and before-images, then checks
them at publication; `session/savepoints.rs` restores a savepoint without
changing the transaction generation. `BEGIN; SAVEPOINT s;` followed by a fork
that stages a previously absent row, `ROLLBACK TO s`, and late fork publication
can pass the existing equal-before-image and generation checks. The regression
must construct this order deterministically, assert no staged row survives,
and prove a fresh post-rollback fork succeeds. No baseline test has run.

This epoch only fences **publication into the parent session**. It cannot undo
or prevent an independently durable `nextval`, a COPY predicate effect, a native
write already submitted, or a later concurrent COMMIT. A savepoint rollback's
post-unlock cleanup still needs its own completion/admission proof. Those
requirements, transferable execution ownership, cancellation and recovery
fencing remain open under #1279 and are not qualified by this increment.

Jev selected the epoch prerequisite 1.0 and identified the source schedule at
0.90, with failed-rollback ordering risk 0.80; receipts live in the preserved
`cassie-sql-copy-admission-1279/target/1279-design/savepoint-fork-{request,response}.json`.
Jev is advisory; source review, focused regressions and the complete gate are
required.
