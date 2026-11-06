# Transaction-owned sequence cursor increment (#833, #839)

The selected Cassie-side increment covers SERIAL and IDENTITY sequences created
in a transactional DDL overlay. It supplies `nextval`, `setval` and sequence
defaults while their new sequence object is still private, and publishes the
latest cursor with the table and sequence metadata in the same existing Midge
family commit. Published sequences retain their existing native, atomic,
retrying write path and nontransactional advancement; unrelated staged DDL must
not change it. No Midge source, new Midge capability, storage representation,
public API, or second storage layer is selected.

The cursor owner is private to the transaction, keyed by the allocated stable
sequence object ID rather than by name, and shared by statement forks. Reserve
retained cursor storage before an identity is inserted; cancellation, admission
failure, and a failed update must not expose a partially installed cursor. A
successful nextval or setval survives statement error and rollback to a
savepoint if the sequence identity existed at that savepoint. A sequence
created after a savepoint becomes inaccessible when that savepoint is restored;
recreating its name creates a distinct identity and starts a fresh cursor. A
full rollback discards unpublished objects and cursor owners. Commit merges
only still-live staged sequence identities into the DDL overlay before the
existing atomic publication and retains no successful result if admission or
publication fails; the failed transaction remains recoverable through the
existing rollback path. Sequence metadata copies must not create a fork
self-conflict, leak a staged object to another session, or overwrite cursor
advances. Destructive object replacement or an identity that cannot be safely
classified as newly staged must not be guessed into the published path.

PostgreSQL 18.6 oracle probes on 2026-11-06 established: a created SERIAL
sequence yields 1, then 2 inside a savepoint, then 3 after rollback to that
savepoint; a rolled-back INSERT consumes 4 and the next value is 5; after
commit the next value is 6. A sequence created after a savepoint, rolled back,
and recreated with the same name starts again at 1. A failed two-row SELECT
that consumed two values reports division by zero, then after rollback to its
savepoint `currval` is 2 and `nextval` is 3. A committed sequence advances from
1 to 2 across rollback with unrelated staged DDL. Raw reference receipts are
in the temporal worktree's ignored `target/scalar-oracle/staged-sequence-*.txt`.

Jev selected the identity-keyed transaction owner over restoring cursor values
from DDL overlay savepoint copies (0.99). It identified identity reuse (0.71)
and failed-statement advancement (0.57); the oracle results above confirm the
required probes. Runtime implementation and all Cassie regressions remain
unexecuted until the assembled build and test gate. This increment does not
complete #833 or #839, or resolve the separately pending RR stale-base catalog
tuple masking semantics.

The published-sequence rename case requires a separate physical-key route: a
new destination's `ExpectedStorageValue::Missing` is insufficient evidence that
its identity is new. Inspect all same-family sequence-key preimages for its
nonzero object ID; fail closed for zero or ambiguous identities. A published
identity must advance at its old physical key using the existing atomic retry
semantics. Under the family `DatabasePublication::with_current_operation` gate,
verify the previous value and identity, reserve all rebase storage before the
native commit, advance and commit, then update the old-key assertion and
renamed destination cursor before releasing the same gate. Later concurrent
writes remain detectable by the final preimage assertion. `OutcomeUnknown`
retains the existing storage-recovery quarantine; never retry or rebase an
unconfirmed write as successful. No partial or unconfirmed allocation may be
reported as a successful sequence value. Lock ordering must be established for
both DDL commit and concurrent calls before enabling that route.

A separate private transaction-owned admission tracker uses the configured
query-memory limit observed at first cursor allocation as its stable cap. Each
new identity first checks current request cancellation and admission, then
grows that shared cap by 2 KiB, including conservative B-tree split overhead.
The tracker is separate from the individual request trackers, so multiple
statements cannot each grow an unbounded retained registry. Savepoint recovery
prepares the live identity list before changing the owner, then releases only
retired identities; the next transaction statement retires dropped objects before its own execution.

The current transaction commit path already clones the entire staged storage
overlay before this increment; that inherited clone is not separately admitted
by this change. The new typed sequence metadata scans check interruption and
admit conservative decode scratch. Cursor-merge serialization counts bytes
before allocating exact-capacity output, and retains each output lease through
publication while the old overlay remains alive. The full pre-existing overlay
snapshot's allocation and lifetime still require a separate bounded-ownership
audit; neither these additions nor source review establish that it is bounded.

This increment routes only `nextval`/`setval` and default advancement to the
new cursor owner. If a private cursor has already advanced, staged ALTER or
RESTART fails with `0A000` before updating sequence metadata; transactional
restart/savepoint semantics remain pending. ALTER before any private cursor
uses the pre-existing staged path. Published renamed sequences also remain unsupported.

COPY FROM has a separate wire/embedded path that currently binds against the
published catalog and does not establish a transaction DDL storage scope. A
newly staged target may fail to bind, and a staged drop/recreate with an
existing name may route to the published object. The attempted target-only
safety comparison did not account for all storage-only changes or prove the
full dependency set; Jev selected a broader guard (0.95), but that would also
reject unaffected published COPY targets after unrelated DDL and is not selected
by the coordinator. COPY target ownership and deterministic safe failure remain
pending #833 work, not a claim of support. Review artifacts and the unexecuted
probe are preserved under ignored `target/sequence-copy-review/`.
