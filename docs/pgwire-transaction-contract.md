# PostgreSQL wire transaction boundaries

Status: implemented finite contract for [#755](https://github.com/cntryl/cassie/issues/755),
with the focused acceptance owners below. Exact-revision repository validation
and adversarial review are recorded in its completing PR. This document does not claim
PostgreSQL server parity. The prerequisite #751 is closed. Selection authority:
[bounded profile](https://github.com/cntryl/cassie/issues/755#issuecomment-5982444561)
and [atomic handoff](https://github.com/cntryl/cassie/issues/755#issuecomment-5994029199).

## Operation boundaries

Classify parsed statements using existing transaction eligibility, rather than
leading keywords. Parse the entire Simple Query before executing its first
statement; bind and authorize each statement at its turn against current state.
The existing Midge mutation stage remains the sole transaction storage owner.
READ COMMITTED is supported; unsupported isolation remains rejected.

| Operation | Selected boundary |
| --- | --- |
| Supported reads and DML | One implicit transaction per Simple Query or extended segment through Sync. |
| Transaction controls | Explicit BEGIN persists across Query and Sync; controls follow the origin table below. |
| DDL/catalog operations rejected in an active transaction | Standalone only. Reject a mixed Simple Query before any execution. Extended standalone DDL must be the first Execute after Sync; require Sync before another Execute. |
| Table COPY FROM STDIN | Preserve the existing single-statement Simple Query streaming path, atomic application when idle, and staging in an explicit transaction. Extended COPY remains unsupported. |
| Database BACKUP/RESTORE | Preserve authorization ordering and idle-only standalone image operations. |
| Parse/Bind/Describe/Close | Preparation alone does not create a mutation transaction. Bound portals still belong to the current segment or explicit transaction. |

Table COPY and database images mixed with other Simple Query statements reject
with `0A000` before execution and without entering copy mode. The generic SQL COPY
guard does not replace the dedicated wire COPY handler. Preserve its 16 MiB
buffer limit, ignored Flush/Sync during copy-in, and late-frame recovery.
No transactional DDL, extended COPY, distributed transactions, new isolation
levels, or persistent transaction format is introduced.

## Origins and protocol recovery

| State | Visibility and ownership | ReadyForQuery |
| --- | --- | --- |
| Idle | No staged transaction; a preparation-only segment may own portals. | `I` |
| Implicit Simple | Ordered statements share private staged writes until Query completion. | Commit or rollback before `I`. |
| Implicit Extended | Executions share private staged writes until Sync or an accepted Simple Query handoff. | Commit or rollback before `I`. |
| Explicit Active | Staged writes persist across Query/Sync and remain private to the session. | `T` |
| Explicit Failed | Further mutations are prohibited; supported rollback/savepoint recovery remains available. | `E` |
| Standalone Complete | Extended standalone DDL has already completed and is visible. Require Sync before another Execute. | `I` at Sync. |
| Discard until Sync | Recovery marker independent of origin. Implicit work is rolled back promptly; explicit failure remains failed. | Exactly one `I` or `E` at Sync. |

CommandComplete and Flush do not publish implicit writes. Commit precedes a
successful ReadyForQuery. Commit failure emits an error and resolves implicit
ownership to idle or explicit ownership to failed; never emit successful ready
with an unresolved commit. Socket loss cannot undo an already completed commit.

## Simple Query transitions

| Start/event | Action and final state |
| --- | --- |
| Idle, eligible success | Begin lazily, execute in order, commit at Query completion, `I`. |
| Implicit origin, eligible failure | Stop at the first error, skip remaining statements, roll back the entire current segment, original error then `I`. |
| Explicit Active, success | Retain staged writes, `T`. |
| Explicit Active, error | Stop immediately, mark failed, `E`. A later textual ROLLBACK in the same frame is not executed. |
| Explicit Failed, non-recovery statement | Reject without mutations, `E`. |
| Whole-frame syntax/resource or unsupported mixed-operation preflight error | Execute no statement from the frame. Roll back an owning implicit segment; fail an existing explicit block. |
| Idle, standalone DDL/catalog | Execute outside transaction staging, existing result then `I`. |
| Explicit origin, DDL/catalog | Reject before mutation; preserve failed-block recovery. |
| Empty Query | Destroy unnamed statement and portal, emit EmptyQueryResponse and one final ready boundary. Finish an owning implicit handoff segment. |
| Table COPY | Complete through its dedicated streaming lifecycle; idle success is `I`, explicit success `T`, explicit failure `E`. |

Every accepted Query has exactly one final ReadyForQuery unless the connection
closes. READ COMMITTED may observe newer committed data between statements;
a suspended portal retains its original snapshot/result and overlay.
The selected shared same-database Data implementation and its pending
qualification are defined in the
[statement Data visibility contract](statement-data-visibility-contract.md).

## Controls inside a cycle

| Origin/control | Transition |
| --- | --- |
| Idle, BEGIN | Start explicit READ COMMITTED transaction, `T`. |
| Implicit, BEGIN | Promote the same stage to explicit: retain writes, conflict intents and original rollback snapshot, `T`. |
| Explicit Active, BEGIN | Preserve unsupported nested BEGIN: `0A000`, `E`. |
| Active implicit, COMMIT/ROLLBACK | Finish current segment and clear its portals. Later eligible statements start a new implicit segment. |
| Explicit Active, COMMIT/ROLLBACK | Finish the block and clear its portals. Later eligible work may start a new implicit segment. |
| Explicit Failed, ROLLBACK | Discard writes, restore session snapshot, release owned resources, `I`. |
| Explicit Failed, supported ROLLBACK TO | Preserve existing savepoint recovery, `T`. |
| Explicit Failed, COMMIT | Preserve rejection `22000`, `E`; require recovery. |
| Idle, COMMIT | Preserve rejection `22000`, `I`. |
| Idle, ROLLBACK | Preserve idempotent rollback and command tag, `I`. |
| Implicit, SAVEPOINT/RELEASE/ROLLBACK TO | Reject `0A000` and roll back the implicit segment. |
| Explicit Active, supported savepoint action | Preserve existing semantics and explicit ownership. |

`BEGIN; INSERT 1; COMMIT; INSERT 2; SELECT 1/0` therefore retains 1 and
rolls back 2. `INSERT 1; BEGIN` retains 1 privately until explicit completion.

## Extended transitions and joined handoff

| Event | Transition |
| --- | --- |
| First eligible Execute | Begin implicit extended ownership unless explicit ownership already exists. |
| Further Execute, resume or Flush | Retain the current stage; emit ordinary results without intermediate commit or ReadyForQuery. |
| Successful implicit Sync | Commit and release segment portals, cursors, reservations and cancellation ownership; one `I`. |
| Explicit Sync | Preserve active/failed block; one `T`/`E`. |
| Extended error | Roll back implicit work promptly or mark explicit block failed; emit error and discard dependent messages until Sync. |
| Recovery marker, Query/Execute/Parse/Bind/Describe/Close | Discard effects until Sync. Flush may flush output but cannot execute or commit. |
| Recovery Sync | Clear recovery marker and segment resources; one `I` or `E`. |
| Standalone DDL as first Execute | Execute immediately and close its execution portal; mark Standalone Complete. |
| Further Execute after standalone DDL before Sync | Reject `0A000`, enter recovery; earlier completed DDL remains visible. |
| DDL inside implicit/explicit origin | Reject before mutation and fail the owning origin. |
| Extended COPY/image operation | Preserve unsupported `0A000` and origin-aware Sync recovery. |
| Terminate/disconnect | Roll back uncommitted work and release execution/resource ownership; no ready response required. |

A healthy unfinished extended segment and an incoming eligible Simple Query
**join atomically**. The Query sees the prefix's staged writes. On success both
prefix and suffix commit before its final `I`; on error both roll back and later
statements are skipped. A subsequent Query can execute before Sync; that later
Sync publishes nothing from the completed handoff. Query BEGIN promotes the
same transaction, including its original rollback snapshot, and returns `T`.
Query COMMIT/ROLLBACK completes the owning segment using the control table.
Unsupported mixed operations are rejected before execution and roll back an
owning implicit segment. An already failed extended cycle continues discarding
Query until Sync; healthy handoff does not bypass that gate.

The streamed standalone DDL exception is deliberate: a later disallowed Execute
cannot retroactively undo DDL whose CommandComplete was already sent.

Parse/Bind/Describe failures and completely framed decoder errors use the same
owning-origin failure law as execution errors. A completed or suspended portal
cannot bypass the shared failed-transaction recovery check when Execute is
retried after recovery Sync. Only the existing ROLLBACK/ROLLBACK TO recovery
statements can execute inside a failed block.

## Object and resource ownership

Named prepared statements belong to the connection and survive transaction
completion until Close or replacement. Every accepted Simple Query destroys the
unnamed statement and unnamed portal, including empty/error Query. Destroying
a statement closes all portals referencing it, including named portals.
An accepted Simple Query ending idle also completes any preceding preparation-only
extended segment and closes its portals, including empty Query and whole-frame
syntax/resource rejection. It does not destroy named prepared statements.

Portals belong to their explicit transaction or implicit/preparation segment.
Close them at owner commit/rollback, preparation-only idle Sync, statement-close
cascade, or disconnect. Suspended portals may resume within that owner. Clients
paging across Sync must use explicit BEGIN; preserve the original snapshot and
cumulative resource limits. Explicit Sync retains live transaction portals.

Cancellation, deadlines, serialization/output-shape failures and resource errors
follow the same origin-specific failure rules. Release rows, cursor/snapshot,
reservations, cancellation registration and worker ownership promptly; stale
resumes must not rerun mutations. Preserve bounded explicit recovery state.

## Finite acceptance and remaining qualification

The focused runtime owners are
[transaction boundaries](../tests/pgwire_extended/transaction_boundaries.rs) and
[portal handoffs](../tests/pgwire_extended/transaction_portal_handoffs.rs), using
[independent raw-wire sessions](../tests/support/pgwire_transaction_boundary.rs).
Prefix CommandComplete is the observer barrier; five-second reads bound every
ready boundary. Each case asserts I/T/E, concrete frame sequences or payloads,
and committed state rather than treating an error response as rollback proof.

| #755 owner | Implemented finite disposition and executed case |
| --- | --- |
| WIRE-009 | Ordered results and stop at first error: `should_roll_back_an_idle_simple_query_batch_on_error` asserts the earlier prefix is absent and later mutation skipped; whole-frame parsing and mixed-DDL rejection execute no frame prefix. |
| WIRE-010 | Implicit Query/Sync ownership: `should_keep_extended_writes_private_until_sync`, joined success/failure, later Sync/no-republication and `should_finish_unsynced_implicit_work_on_simple_commit_or_rollback`. Same-stage BEGIN retains writes and its original session rollback snapshot; explicit COMMIT can end one segment before a later failed implicit segment. |
| WIRE-015 | Direct unnamed statement Describe and unnamed portal Execute after accepted Query; named prepared statements survive, portals derived from destroyed statements close. Empty/error Query and preparation-only normal/recovery Sync close idle segment portals. |
| WIRE-022 | `should_discard_simple_query_messages_after_failed_extended_work_until_sync` proves skipped mutations, one recovery ready and a healthy sentinel. Original parse-error pipelining owners remain preservation controls. |
| WIRE-024 | Explicit failed E persists through Sync until ROLLBACK; implicit commit failure returns its original 23503 and I at the same Sync. Read committed succeeds; repeatable read/serializable and nested explicit BEGIN remain rejected. Output-limit failure produces E in an explicit block; revoked COPY access rolls back implicit ownership to I. |

Standalone extended DDL first/second Execute, mixed Simple DDL, explicit table
COPY commit/rollback and implicit COPY rejection have separate named cases.
Disconnect acceptance observes zero sessions, prepared statements, portals,
accounted query memory and operator workers before querying committed state
from a new independent connection. Existing snapshot, retained-memory, result-cap,
completion/non-reexecution and cancellation owners in `tests/pgwire_extended.rs`
now explicitly BEGIN when retaining a portal across Sync.

The completing PR retains genuine red/green transcripts and exact validation
commands; passing these cases does not prove every protocol interleaving.
Broader snapshot/resource qualification remains [#757](https://github.com/cntryl/cassie/issues/757),
transaction publication/visibility qualification [#763](https://github.com/cntryl/cassie/issues/763),
and packet/client workflow qualification [#780](https://github.com/cntryl/cassie/issues/780).
No stronger isolation, transactional DDL, distributed transaction or general
PostgreSQL server-parity claim follows from this finite acceptance.
