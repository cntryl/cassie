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
| Selected schema/table/scalar-index DDL | Explicit transactions only. The session reads its staged objects; commit publishes them atomically with row writes. See the #833 extension below. |
| Other DDL/catalog operations | Standalone only. Reject a mixed Simple Query before any execution. Extended standalone DDL must be the first Execute after Sync; require Sync before another Execute. |
| Table COPY FROM STDIN | Preserve the existing single-statement Simple Query streaming path, atomic application when idle, and staging in an explicit transaction. Extended COPY remains unsupported. |
| Database BACKUP/RESTORE | Preserve authorization ordering and idle-only standalone image operations. |
| Parse/Bind/Describe/Close | Preparation alone does not create a mutation transaction. Bound portals still belong to the current segment or explicit transaction. |

Table COPY and database images mixed with other Simple Query statements reject
with `0A000` before execution and without entering copy mode. The generic SQL COPY
guard does not replace the dedicated wire COPY handler. Preserve its 16 MiB
buffer limit, ignored Flush/Sync during copy-in, and late-frame recovery.
COPY scalar input preserves the selected type parser's diagnostics: malformed
integer, FLOAT8, Boolean, NUMERIC, JSON/JSONB and OID input reports `22P02`,
and integer/FLOAT8 range errors report `22003`. CSV row-width mismatch and
unterminated quoting report `22P04`. A failed batch publishes none of its earlier
rows, emits no successful command tag, and returns to the appropriate ready
state. Focused probes are in
[`errors.rs`](../tests/compatibility/lifecycle/copy/from_stdin/errors.rs).

No extended COPY, distributed transactions, new isolation levels, or persistent
transaction format is introduced.

## Selected READ COMMITTED unique-owner wait slice

The scoped runtime increment for [#819](https://github.com/cntryl/cassie/issues/819)
tracks non-null values from single-column PRIMARY KEY and UNIQUE constraints
staged by plain SQL `INSERT` in an explicit transaction. A competing plain SQL
`INSERT` for the same key waits until its owner commits, rolls back, disconnects,
or rolls back to a savepoint that removed the staged insert. The waiter releases
collection write gates while waiting, then rechecks resolved candidates against
gated live constraint state before waiting on another owner or publishing. A
committed duplicate returns `23505` without waiting on unrelated unresolved
keys. If the owner rolls back, the waiting `INSERT` can stage and commit without
evaluating defaults or expressions a second time.
Savepoint rollback releases keys acquired after that savepoint. Waits honor
query cancellation and deadlines. Detected wait cycles return `40P01`; the
victim's previously staged keys remain owned until transaction resolution.

The next bounded increment extends this ownership protocol to explicit
single-column PRIMARY KEY/UNIQUE `ON CONFLICT (field)` targets with stable
integer, Boolean, text/varchar, UUID or BYTEA key representations and actions
that preserve the arbiter field, on tables without vector indexes. Candidate values, identity and defaults are
prepared once under admitted memory. Waiters release collection gates, honor
cancellation/deadlines, and recheck current READ COMMITTED data before applying
DO NOTHING/DO UPDATE and producing the final RETURNING row. Failed statements
and autocommit completion release their claims; successful explicit writes
retain ownership until their transaction resolves. This increment is selected
for projector idempotency on 2026-10-24. Local probes cover owner commit and rollback,
single evaluation of defaults, cancellation, false action filters, non-arbiter
uniqueness, preservation of earlier transaction keys, and failed multi-row
atomicity in [`upsert_concurrency.rs`](../tests/compatibility/lifecycle/mutations/upsert_concurrency.rs).
This evidence does not qualify the broader transaction contract.

The ownership registry is private and in-memory. This slice does not select
REPEATABLE READ or SERIALIZABLE, and it does not qualify unique indexes,
composite or expression constraints, or wait behavior for UPDATE, MERGE, COPY,
broader `INSERT ... ON CONFLICT` targets, or other write paths. Those obligations remain open
under #819 and its named downstream owners. Focused executable evidence is in
[`transaction_errors.rs`](../tests/sql_mutations/transactions/transaction_errors.rs)
and the completing PR; the implementation does not complete #819.

## Selected local-development contract expansion

The table above records the finite #755 wire boundary. The local-development
gate selects the following database-scoped transactional DDL extension under
[#833](https://github.com/cntryl/cassie/issues/833). Its selected runtime slice
is implemented in the current PR; the broader issue acceptance and protocol
qualification remain open:

- SQL materialized-view `ALTER ... RENAME TO` uses the same catalog overlay,
  retaining physical output/version identity and rejecting unsupported stored
  dependency rewrites before mutation.
- Scalar `ALTER INDEX ... RENAME TO` preserves stored identity and renames owning
  key constraints within the same catalog overlay; savepoint/transaction rollback
  restores both names. Storage-parameter SET/RESET retains its existing transaction
  restriction.
- Supported schema, table, and scalar-index CREATE/ALTER/DROP operations stage
  catalog and row changes within the owning Midge database family.
- The owning session sees its staged objects and can read or write through them.
  Other sessions see the previous committed catalog and rows until commit.
- Commit publishes catalog changes and staged row writes atomically in that
  family. Rollback discards both. Savepoint rollback restores the catalog and
  row state that existed at the savepoint.
- A failed DDL or constraint check aborts the explicit transaction until
  rollback or rollback to a valid savepoint. Failed changes do not reserve
  relation or index names after recovery.
- Schema/data changes invalidate affected prepared plans and cached results at
  publication. Rolled-back changes never become visible to another session.
- Cross-database transactions, broader isolation levels, and persistent format
  changes remain outside this selected contract.

Focused acceptance includes create/evolve/populate, deliberately failing
migrations and writes, cross-session visibility, savepoint and transaction
rollback, named constraint errors, index contents, and restart readback. Broader
catalog snapshot semantics remain with their existing owners; this contract
does not select full PostgreSQL catalog MVCC.

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
| Explicit Failed, non-recovery statement | Reject `25P02` (current transaction is aborted) without mutations, `E`. |
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
| Explicit Failed, COMMIT | Roll back the failed block as PostgreSQL does: no error, command tag `ROLLBACK`, `I`. |
| Idle, COMMIT | Preserve rejection `22000`, `I`. |
| Idle, ROLLBACK | Preserve idempotent rollback and command tag, `I`. |
| Idle, SAVEPOINT/RELEASE/ROLLBACK TO | Reject `25P01` (can only be used in transaction blocks), `I`. |
| Implicit, SAVEPOINT/RELEASE/ROLLBACK TO | Reject `25P01` and roll back the implicit segment. |
| Explicit Active or Failed, RELEASE/ROLLBACK TO missing savepoint | Reject `3B001`; the block is or remains failed, `E`. |
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
| DDL inside an implicit origin, or unselected DDL inside an explicit origin | Reject before mutation and fail the owning origin. |
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
[transaction boundaries](../tests/pgwire_extended/transactions/transaction_boundaries.rs) and
[portal handoffs](../tests/pgwire_extended/transactions/transaction_portal_handoffs.rs), using
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
No stronger isolation, distributed transaction or general PostgreSQL
server-parity claim follows from this finite acceptance.
