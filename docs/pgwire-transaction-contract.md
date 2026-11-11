# PostgreSQL wire transaction boundaries

Status: implemented finite contract for [#755](https://github.com/cntryl/cassie/issues/755),
with the focused acceptance owners below. Exact-revision repository validation
and adversarial review are recorded in its completing PR. This document does not claim
PostgreSQL server parity. The prerequisite #751 is closed. Selection authority:
[bounded profile](https://github.com/cntryl/cassie/issues/755#issuecomment-5982444561)
and [atomic handoff](https://github.com/cntryl/cassie/issues/755#issuecomment-5994029199).

The initial sections record the completed #755 baseline; later sections describe
the selected v0.2 extensions and their qualification status. References to Midge
0.5.1 in those historical checkpoints describe the dependency at that time.
The combined #1184 candidate uses published Midge 0.5.2 and requires a new gate.

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
| Table COPY FROM STDIN | Preserve the existing single-statement Simple Query streaming path, atomic application when idle, and staging in an explicit transaction. Under REPEATABLE READ, COPY into a published table dropped and recreated with the same name in that transaction returns `0A000` before CopyInResponse and leaves the explicit transaction failed until rollback. READ COMMITTED replacement, new transaction-owned names, and unchanged published targets retain their selected behavior. See the [2026-11-09 decision](transaction-owned-copy-2026-11-06.md#2026-11-09-recreated-published-targets-under-repeatable-read). Extended COPY remains unsupported. |
| Database BACKUP/RESTORE | Preserve authorization ordering and idle-only standalone image operations. |
| Parse/Bind/Describe/Close | Preparation alone does not create a mutation transaction. Bound portals still belong to the current segment or explicit transaction. |

Table COPY and database images mixed with other Simple Query statements reject
with `0A000` before execution and without entering copy mode. The generic SQL COPY
guard does not replace the dedicated wire COPY handler. Preserve its 16 MiB
buffer limit, ignored Flush/Sync during copy-in, and late-frame recovery.
The aggregate bound covers all `CopyData` frames in one stream; exact-limit and
one-byte-over boundary evidence is in
[`payload_limit_tests.rs`](../src/pgwire/connection/copy/payload_limit_tests.rs).
`CancelRequest` wakes an idle `COPY FROM STDIN` read and returns SQLSTATE `57014`
without waiting for another frontend frame. An implicit COPY returns to ReadyForQuery
with status `I`; cancellation inside an explicit transaction returns status `E`.
Late `CopyData`, `CopyDone`, and `CopyFail` messages are discarded, after which an
explicit transaction can be rolled back and the connection can continue querying.
Cancellation interrupts while the handler waits for the next frontend tag. After
consuming a tag, the handler finishes that frame before applying cancellation, so
dropping a read cannot lose the rest of a partially received frame.
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

## Projector concurrency boundary for v0.1.0

The MVP retains the wait behavior described above. No selected v0.1.0 workflow
requires overlapping conflicting writes on the unqualified key shapes or paths
listed above. The qualified wait applies only where both the owning write and
the competing write meet the documented slice. Otherwise, coordinate all writers
to the affected table, including non-projector and COPY clients, from transaction
start through commit or rollback. The safe default is one writer per affected
table; use finer partitioning only when external coordination guarantees that
all old and new unique keys touched by concurrent transactions are disjoint.
That external coordination is not a claim that broader concurrency is qualified.
Serializing statement submission alone is insufficient. Treat statement success
and RETURNING values as provisional until the transaction commits. An
unqualified competing operation may return before its owner resolves and may
fail at commit.

This operating boundary does not assert that unsupported concurrent cases are
rejected or that their final results are qualified. Scalar-index lifecycle
coverage does not establish concurrent conflict-wait semantics. Wider waits
and snapshot isolation remain open under #819, outside the v0.1.0 milestone.
Representative concurrency qualification remains a v1.0 obligation.

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

### Selected standalone table publication correction (2026-11-04)

[#1157](https://github.com/cntryl/cassie/issues/1157) selects a bounded correction
for ordinary standalone `CREATE TABLE`, including `IF NOT EXISTS`. Its implicit
catalog snapshot, staging and commit share the existing exclusive catalog
publication guard, with interruption checks while waiting. The guard stays on
the synchronous execution worker and does not cross a network await. Commit
precedes this standalone statement's `CommandComplete` in both Simple Query and
extended execution. `CREATE TABLE AS` keeps its separate existing execution path.
This ownership decision is made at the top-level wire entry. CTAS's internal
table creation and other nested bound operations retain their enclosing owner.
CTAS's `SELECT n` completion retains the affected-row count supplied by the
operator; ordinary result sets and portal pages report their returned-row count.

An unchanged catalog snapshot with no staged storage metadata does not publish,
advance the schema epoch or conflict merely because another session published
unrelated metadata. Genuine explicit transactional DDL retains its version
conflict check; Cassie does not replay explicit user transactions. The concurrent
cold/warm, no-op, conflict, cancellation and nested CTAS probes belong to the
coherent bundle's executable gate; executed results are recorded in its PR.
This contract selection alone does not claim container publication.

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


## Selected v0.2 snapshot profile (2026-11-04)

The user approved this contract direction on 2026-11-04. It is selected
implementation scope, not implemented or qualified support. Existing rejection
and compatibility boundaries remain until the owning acceptance cases pass.

### #819

Keep READ COMMITTED as default. Add REPEATABLE READ for read-only and read-write
transactions within one database family. Acquire a retained data/catalog view on the
first snapshot-taking statement, overlay own writes, and retain it through transaction
completion and suspended portals. Savepoint rollback restores own writes/catalog changes
without advancing the snapshot. Use first-committer-wins write validation: a write based
on a row changed since the retained snapshot fails with 40001 before publication. Reject
SERIALIZABLE and DEFERRABLE in the new RR profile with 0A000. Preserve the existing
READ UNCOMMITTED-to-READ COMMITTED alias and its fresh command snapshots; adding RR
does not remove that supported behavior. Do not claim predicate-serializable guarantees. Finish owner-wait/error-phase semantics
for the selected scalar primary/unique shapes across supported INSERT/UPDATE/ON
CONFLICT/COPY paths before claiming unrestricted projector concurrency.

Use direct existing Midge statement/snapshot primitives plus a transaction-owned
retained view; no durable snapshot image or new Midge API is assumed. Prove Midge can
retain the required view before runtime work; if not, record the upstream dependency.
Bound retained resources/wait graph, release on commit/rollback/disconnect/cancel,
report 40P01 for detected wait cycles and 57014 for cancellation. Existing transaction
error and ReadyForQuery carriers remain authoritative. No persistent format change is
needed for isolation itself.

### Explicit nullable columns

`CREATE TABLE` and `ALTER TABLE ADD COLUMN` accept explicit `NULL` column syntax,
including repeated or named NULL clauses. The clause is equivalent to omitted
nullability; it creates no persistent constraint identity. Conflicting NULL and
NOT NULL, inline PRIMARY KEY, SERIAL or IDENTITY declarations return SQLSTATE
`42601` before publication. A table-level primary key may supply NOT NULL after
an explicit nullable column, matching PostgreSQL. A later ALTER may also add
NOT NULL to the originally nullable column.

### RR snapshot acquisition and catalog authority

Pinned PostgreSQL 18.6 probes on 2026-11-04 distinguish transaction ownership
from snapshot acquisition. Inside an already-explicit RR block, a qualifying
Parse or Bind may acquire the first snapshot before Execute. Executing constant
`SELECT 1` also acquires it. SHOW, SAVEPOINT and Describe alone do not acquire
the data view. Idle and READ COMMITTED preparation-only mutation ownership stays
unchanged. The selected implementation follows these observed boundaries.

SQL catalog object rows use the retained metadata view. Ordinary descriptor/name
lookup and authorization remain current: the native oracle excludes a newly
created table from `information_schema.tables` in the older RR snapshot, yet a
direct SELECT from that table resolves and returns its snapshot-visible empty
result. Existing dynamic settings and session-object catalog branches remain live.

The selected owner is a transaction-retained Arc over the existing published
Midge statement-read owner and a separately admitted controlled Catalog snapshot.
Its lifetime is independent of the first Parse/Execute request guard and of
savepoint rollback. Query/portal owners may share it; the last transaction/portal
owner releases its charge. Explicit-error recovery retains only the bounded
transaction snapshot needed to keep ROLLBACK TO on the same view; statement
resources still retire on error/cancellation.

Jev selected the observed explicit-RR acquisition classification (0.93), separate
SQL catalog/current descriptor authorities (0.96), and transaction Arc ownership
(1.00). The controlled detached Catalog approach was selected at 0.97 after
reviewing its 22 typed maps and current cache-backed hydration callers. The 0.86
uncharged-clone and 0.94 JSON-length-only accounting leads require typed admission
and ownership probes before qualification. These are advisory decision records;
the source, pinned oracle and executable tests establish acceptance.

The first runtime increment may qualify read-only RR independently. Read-write
RR remains rejected until per-row first-committer-wins validation, disjoint writers,
ABA behavior and the selected owner-wait/error phases pass on published Midge
0.5.1. No Midge engine/source/registry/dependency change is selected.

Snapshot acquisition uses PostgreSQL 18.6's phase distinction. Execution defaults
utility statements to snapshot-taking except transaction controls, SET, SHOW,
and FETCH/MOVE among Cassie's supported statements. Parse uses the upstream
analysis classes (queries/DML, EXPLAIN, DECLARE, CALL, and CREATE TABLE AS); Bind
uses those classes or parameter input. Describe does not acquire a view. Pinned
wire probes confirm constant SELECT, Parse, Bind, PREPARE, DECLARE, EXPLAIN,
CALL, and DEALLOCATE capture before a competing commit; SHOW/SAVEPOINT and
Describe do not. Jev selected this phase-aware classification (0.93) and blocking,
cancellable Parse admission (0.90); these are advisory choices backed by the
upstream functions and executable probes, not support proof by themselves.

Read-write integration is still an explicit gap, not an upstream capability
absence. Published Midge 0.5.1 retained assertions can detect ABA and allow
disjoint rows, as the adapter capability probes demonstrate. The proposed pair
of retained read/write transactions cannot be assumed to share one snapshot:
current collection gates do not cover the public native `database_tx` and raw
write paths. Further, replaying writes against the older transaction without
fresh metadata handling can overwrite a newer shared data epoch, index state,
or collection generation. A whole-database epoch assertion would reject valid
disjoint writers and is not the selected contract. Jev selected retaining the
read-only boundary while recording this integration gap (0.99), with a material
candidate gap of 0.84. Authored native probes cover separately captured views
and rejection of newer expected values against older assertion snapshots; their
execution is deferred to the frozen bundle gate. A future Cassie-only design
may investigate a single retained RW read owner, fresh operation preparation
and replay, portal retirement, and complete publication/epoch synchronization.
It must be proved before read-write RR is enabled; Midge changes remain excluded.

Runtime scalar access paths also respect the retained view. A current index
descriptor can refer to a prefix created after the data snapshot, so scalar
selection, bounded indexed joins, and full-text scalar prefilters must decline
that index and use their existing row or exact fallback. Eligibility compares
borrowed read definitions and the current/retained nonzero collection identity.
When current index IDs are present they must match the retained IDs; logical
SQL catalog entries can omit them, in which case the existing native resolver
loads both from retained Data metadata. Identical names/definitions are not
sufficient for a newly recreated table with a different collection identity.
This does not replace current binding or shared plan-cache inputs. Jev selected this narrow eligibility contract at 0.99. The 0.60 identity
lead is covered by authored unchanged/replaced/renamed, REINDEX, and drop/recreate
controls, pending the frozen executable gate. Existing column-batch,
time-series, full-text artifact, and ANN readers have separate missing-artifact
or snapshot-generation fallback checks; their source inventory is retained in
the validation record.

Encoding descriptor authority is captured in the read owner. Ordinary READ
COMMITTED statement owners preserve their existing frozen Data metadata law;
RR owners use current encoding descriptors with historical rows. The immutable
owner distinction survives worker/cursor sharing. Jev selected it at 0.99 after
identifying the unconditional-fresh descriptor regression at 0.88; authored RC/RR
controls and current rename/add-column reads await the frozen executable gate.



### Next read-write design gate

The next selected investigation is a Cassie-side publication gate per database
family. Every publishing `AdapterTransaction::commit` must participate, including
SQL/REST document mutations, graph/time-series/fresh loads, DDL backfills and
maintenance sidecars. Pure overlay staging does not publish. Capture the retained
RW assertion fence and readonly data view while that same gate is held, under
the catalog publication boundary. Existing per-collection and referential gates
remain necessary for preparation and constraint ownership. The read-only
baseline had no general database publication gate; the common Cassie-only guard
is now authored as a prerequisite, pending the coherent RW executable gate.

Jev selected investigating assertion-only old-start row validation followed by
existing fresh atomic document publication while the complete gate remains held
(0.99), rather than adding a controlled operation recorder/replay path. Published
Midge 0.5.1 explicitly validates assertion-only commits server-side. The first
native commit writes no SQL data; the second reuses fresh uniqueness, index,
generation and data-epoch preparation. A competing protected writer must not
publish between them. This is a design selection, not an implemented guarantee:
the separate conditional race/atomicity lead remains 0.74 and requires both
native assertion-only ABA/disjoint probes and synchronized competing-writer
coverage of the complete publication window. No whole-database epoch assertion
is selected.

Both commit strategies consume the retained native fence. The selected lifetime
for a failure during a new RW RR COMMIT attempt is to retire the entire block and
its owners to Idle, matching PostgreSQL 18.6 `AbortCurrentTransaction`'s
`TBLOCK_END` branch (Jev 0.88). Errors in earlier ordinary statements still retain
the bounded snapshot/fence for savepoint recovery. Existing READ COMMITTED
behavior is not changed by this investigation.

The supported SQL/projector/document mutation APIs and public raw point writes
must all be covered by the private observed adapter path. A returned native
read-write transaction cannot be observed at its later commit. The selected
interop boundary is therefore mutually exclusive for each Midge instance: once
any public native writable transaction has been returned, new read-write RR
capture fails with `0A000` until that instance is closed; once tracked read-write
RR has been admitted, subsequent public native writable transaction factories
fail with `0A000` for the rest of that instance's lifetime. One private admission
lock protects both transitions. No public reset knob is added. Read-only RR,
READ COMMITTED, read-only native transactions and ordinary private SQL/REST
writers remain available, subject to normal recovery admission. Jev selected
this source-compatible fail-closed policy at 0.71.

The exact public native factories are `schema_tx`, `data_tx`, `database_tx`,
`temp_tx`, `default_tx` and `begin_families_tx`. `Midge.engine` is private to the
adapter; Midge exposes no native Engine Arc/reference or dereference. Public
`database_family`, `storage_layout`, `bootstrap_families` and
`ensure_families_ready` return native column-family handles whose published
0.5.1 API is only `new`, `id` and `name`; they cannot create a writer. A native
read-only transaction rejects `put`, `insert`, `delete` and `delete_range`, and
has no mode-changing API. The internal AdapterTransaction mutable dereference
must be removed so every authoritative mutation uses an explicit observed
wrapper. Internal initialization, SQL/REST, cache and cleanup writers use private
factories and must not latch native-escape mode. New native handle APIs require
the same capability audit before they can participate in this profile.

Common obligations remain: per-command own-write overlays over one retained
committed view, own catalog changes over the original catalog snapshot, precise
unique/ON CONFLICT owner waits and invisible-target errors, bounded resources,
legacy and column-store row identities, and complete savepoint/portal/error
lifetimes. In particular, current RC conflict-intent resolution must not silently
retarget an RR upsert to a row absent from its snapshot. RW remains rejected
until the selected obligations and frozen executable gate pass.

The publication gate must survive an unknown native commit outcome. Midge 0.5.1
states that an admitted timed-out write may still apply after the caller returns;
a mutex released at that return alone cannot fence later publication. The
selected Cassie guard therefore latches an instance-wide recovery-required flag
before releasing the gate after any native `OutcomeUnknown`. New publishing
operations and new RR snapshot acquisition reject until that Midge instance is
closed and a new instance completes normal recovery. Existing retained reads and
transaction cleanup may still retire their owners. A weak gate cache must not
clear the recovery-required state. Known-not-applied conflicts or stalls do not
set it.

The blocked operation uses a private adapter error mapped to existing terminal
storage SQLSTATE `58030`; its message states that an earlier write is unconfirmed,
that the new commit was not submitted, and that recovery/reconciliation is
required. It is not reported as a new unknown-outcome write or a retryable stall.
Jev selected the instance lifetime and this diagnostic contract at 1.00 each. The
remaining conditional race lead is 0.30 and requires synchronized late-outcome,
blocked-capture/blocked-publication, known-not-applied, gate-owner retirement and
recovered-instance controls before the guard can qualify RW.

Publication waits use the current blocking request's private control scope
(Jev 0.97), with interruption checks while waiting and immediately before native
submission. A retained RW fence does not retain the cancellation token from its
initial Parse or Execute. Nested SQL/document calls inherit the originating
request; standalone administrative document/COPY calls establish their current
cancellation/deadline owner. The scope never crosses an async suspension.

The publisher inventory also includes database lifecycle journal transactions,
unpublished database import transactions, bootstrap layout writes, Temp cleanup
and vector staging cleanup. Their internal native transactions participate in
the same private adapter outcome/gate handling. Native column-family create/drop
outcomes latch recovery-required state too; an unknown create outcome must not
be converted to success by an existing-family lookup fallback.

Quarantine also blocks new reads and metadata admission. New ordinary SQL
Parse/Describe/Bind/Execute, cached results, suspended portal resumes, SQL cursor
FETCH, and ordinary REST routes reject with terminal `58030`/HTTP 503. Fresh
private adapter read acquisition rejects too. A request accepted before the
latch may finish only from its already pinned immutable data and owned encoding
descriptors/results; a fresh descriptor/read acquisition can fail it. No new
Execute is permitted merely because a portal retains a cursor or buffered rows.
Any unknown-outcome diagnostic returned to the caller retains its original
write outcome. Existing best-effort Temp cache cleanup may still ignore its
error for an already accepted read, while the instance remains quarantined;
subsequent fresh reads and publications reject. This preserves the selected
owned-response completion boundary without exempting cache commits from latching
(Jev 0.77).

Plain full ROLLBACK/ABORT remains local cleanup after quarantine; once classified,
it retires transaction owners before response allocation. COMMIT or a chained
transaction-end attempt retires the block to Idle and reports the terminal
unsubmitted-operation error; it cannot publish or start a replacement block.
Terminate, EOF and protocol synchronization remain available for cleanup.
Healthy transaction behavior is unchanged. Jev selected these SQL cleanup and
in-flight boundaries at 1.00 each.

The REST exception is an explicit GET-only public health/liveness allowlist:
`/health`, `/healthz`, `/readyz`, `/startupz`, `/liveness`, `/livez`, and `/targetz`.
Post-quarantine readiness uses only in-memory/cached observations and is false;
process liveness may remain true. `/metrics`, authentication, administrative,
static/UI and ordinary data routes are not exceptions: existing stored-token
authentication and capacity reads cannot be assumed safe after uncertain
metadata publication. Admission must run before auth storage access and again
when a worker begins after body awaits. Jev selected this finite allowlist at
0.97. Closing/reopening the storage owner through normal recovery is required;
no public reset endpoint or flag-clear operation is added.

The common implementation keeps SQL controls and the originating REST worker's
absolute deadline/cancellation in one fixed, stack-scoped pair. Nested SQL or
document helpers cannot replace the REST deadline with a later SQL deadline.
Blocked publication and snapshot-window acquisition check both authorities
before waiting, while contended, and immediately after acquiring the gate. No
first-Parse/first-Execute cancellation owner is retained in the publication gate.



### Read-write command overlay ownership

The private read owner separates one immutable committed-data core from each
command's COW own-write overlay (Jev 0.97). The core retains the same published
native transaction, engine, family identity, epoch and descriptor authority. Each
new wrapper and overlay has its own admission; shared backing remains charged
until its last owner. Read-only RR continues to reuse its whole initial owner
without charging a second request. A RW command shares the committed core and
captures the current own-write map while the session transaction lock is held.
Savepoint restoration changes future command maps while existing portals retain
their original command map. RW mode remains disabled until its commit fence and
remaining acceptance obligations are complete.

A modifying CTE's atomic working session has a different identity from its
originating session. Its SQL source reads must retain the original command's
COW map rather than recapturing earlier sibling CTE effects. A private rebind
requires the owner to match the original session and Midge/database, then
rebinds only that admitted immutable map to the working identity. The rebound
controls flow through the existing query-read boundaries for ordinary atomic
DML and the simple upsert wait path. Constraint and mutation validation retains
its live own-write behavior; a global rebound thread-local scope would wrongly
hide a parent inserted earlier in the same command from a foreign-key check.
Jev selected these existing read boundaries at 1.00 after source inspection;
no global session-identity check is weakened.



### Selected row-fence integration

The new RW owner retains a separately admitted private boxed read-write native
transaction, using its old start sequence and `LastWriteWins` only for the
assertion-only validation step. It is captured with the immutable Data view
under the same Cassie database publication gate. The fence owns its engine Arc
and narrow memory lease; it retains no request cancellation/deadline token.

For each final changed row, validate the modern row key, the column-store row
marker, and the legacy document key against that retained native start. The
current/own-DDL catalog supplies the relation identity without cloning a large
encoding descriptor. If any representation existed in the retained view, all
three are asserted, including the other absent slots; a marker rewritten with
the same value still conflicts through the published start-sequence check. If
all three were absent, compare those keys with a fresh same-family read under
the complete publication window. Any current representation rejects with
`40001`; if all remain absent, allow the new row. Normal document publication
is an upsert, so uniqueness alone cannot protect a newly staged row from a
later direct document writer using the same physical row identity. This bounded
mixed-interface overlap fails before user publication (Jev selection1.00,
error identity0.98), without claiming PostgreSQL RETURNING/wait phase for a
direct document API. PostgreSQL 18.6 permits a new insertion after another transaction's
completed insert/delete against an originally empty view; an old missing-key
assertion would incorrectly reject that case. The six pinned PK/UNIQUE controls
supersede the earlier blanket-absence design judgment; Jev selected this
correction at 1.00. Do not assert the shared data epoch or unrelated rows.

Cassie admits its boxed owner and constructed key/value input buffers against
the current commit request; the native assertion pool remains independently
bounded by Midge (selected at 1.00). Borrow final staged identifiers rather than
cloning their JSON payloads. Acquire existing referential/collection gates before
the database publication window, then keep that window through assertion-only
validation and fresh normal atomic publication. Every new RW COMMIT attempt
failure retires the block to Idle and returns its original error; earlier
statement errors keep the snapshot/fence for savepoint recovery (selected at
1.00). This is an implementation contract; RW support remains unselected at
runtime until the complete accepted behavior and executable gate pass.


### Selected early conflict authority

Pinned PostgreSQL 18.6 rejects an actual current PK/UNIQUE conflict target
changed after the retained view with `40001` at INSERT, before RETURNING or
DO UPDATE filter evaluation. The 24-case oracle includes same-payload UPDATE,
changed UPDATE, delete/reinsert, target absent from the older view, DO NOTHING
and DO UPDATE WHERE FALSE. A final changed-row fence alone cannot cover these
no-write actions or the required error phase. The six current-empty controls
allow a new INSERT after another transaction's completed insert/delete, so the
check applies to the resolved current target, not every formerly absent key.

The selected Cassie-only supplement is bounded process-local authoritative-key
mutation history (Jev 0.93), with no persisted format or Midge change. Under the
complete database publication gate, a RW snapshot registers the current checked
local publication sequence. Every private adapter point put/delete records
modern row, column-store row-marker and legacy document keys even before any RR
owner exists, because a writer can stage before capture and commit afterwards.
Same-value puts count as mutations. Deferred overlays publish no history until
the actual native transaction publishes them.

Before native publication, admit all history growth. Install confirmed mutation
sequences infallibly after native success and before releasing the same gate;
known failures install nothing, and unknown outcomes quarantine the instance.
A resolved conflict target whose key changed after the retained sequence fails
before either conflict action/filter/RETURNING, with an exemption for the
transaction's own staged row. Keep the old-present native final written-row
fence as a separate first-committer-wins authority. RR conflict-intent resolution
must not take READ COMMITTED's fresh-target retry path.

A dedicated bounded budget per physical database family, configured with
`CASSIE_REPEATABLE_READ_HISTORY_MEMORY_BUDGET_BYTES` (default 10 MiB), owns
stage-key logs, history keys,
container backing, active registrations and publication preparation overlap
(Jev 0.86). It retains no request cancellation/deadline. Exhaustion returns
`54000` before staging/publication; no required live history is evicted. Sequence
exhaustion also fails before publication. Registration/drop and pruning use the
same family owner and lock ordering; release history after the last relevant
snapshot. New physical families/restores establish a new baseline, while renames
preserve stable modern relation identity and log any legacy-key rewrites.

This amendment selects implementation work, not support qualification. RW stays
rejected until early-phase/ABA/no-write, disjoint writer, native-interop,
resource, savepoint/portal and recovery obligations pass the frozen full gate.



The private history uses sorted capacity-coupled vectors (Jev 0.99), with
pre-admitted replacement overlap through the existing retained-memory helper.
Only keys absent from the current history require new history slots; rewriting
an existing key must reuse its slot. Stage-key ownership moves into confirmed
history without copying after native success. The independent tracker itself is
created lazily, only after the complete header allowance is checked. The private
mutation error carrier preserves native failures and maps new accounting
failures to the existing typed resource error (`54000`); Jev selected this
caller-preserving refinement at 0.99 after the exact conversion/lint inventory.
Authored confirmation-time budget-exhaustion and full-directory reuse probes
remain pending the frozen gate, along with all other new RW probes.



A local close/reopen mode change also requires the published engine's fencing
resources to retire. Midge0.5.1 owns a runtime transaction guard in each returned
native transaction and moves its runtime plus directory/cloud lease into a
reaper when Engine drops. The lease stays owned until those old transactions
and workers retire. An authored explicitly local recovery control retains a
returned writer across Midge drop, requires another open to fail, then retires
the old handle and permits a recovered instance to select tracked mode. This is
pending executable evidence; dropping the Cassie wrapper alone is not a claim
that a still-live external native transaction has disappeared.



### Explicit nullable columns

`CREATE TABLE` and `ALTER TABLE ADD COLUMN` accept explicit `NULL` column syntax,
including repeated or named NULL clauses. The clause is equivalent to omitted
nullability; it creates no persistent constraint identity. Conflicting NULL and
NOT NULL, inline PRIMARY KEY, SERIAL or IDENTITY declarations return SQLSTATE
`42601` before publication. A table-level primary key may supply NOT NULL after
an explicit nullable column, matching PostgreSQL. A later ALTER may also add
NOT NULL to the originally nullable column.


### Selected bounded own-DDL catalog profile

RR SQL catalog rows compose the initial metadata view with this transaction's
selected DDL. A later current catalog overlay cannot replace the initial view:
that would import unrelated committed objects. The selected implementation uses
an admitted immutable DDL base and an exact private content comparison over the
thirteen maps published by `Catalog::publish_transaction_snapshot`. A controlled
delta copy inherits all twenty-two initial maps and applies only this
transaction's insertions, replacements and removals in those thirteen maps.
Container capacity affects admission and does not affect metadata equality;
floating metadata fields compare their bits. No public comparison trait is
added (Jev selection0.99).

The bounded RR catalog-write rule is deliberately conservative: every catalog
write-intent statement checks the current thirteen-map metadata against the
initial/own stable base, including referenced schemas and dependencies rather
than just keys it would change. A stale base returns `40001` before binding or
execution, including a stale `IF NOT EXISTS` attempt (Jev selections0.98 and1.00).
READ COMMITTED retains its existing behavior. Pure DML first-committer-wins
validation still permits disjoint row writers and never asserts a shared data
epoch. Ordinary descriptor/name lookup and authorization remain current,
layered with the transaction's own DDL.

This does not claim PostgreSQL's internal catalog-tuple MVCC. Pinned PostgreSQL
18.6 permits ALTER of a table created outside the RR view and can expose only
its own added attribute while retaining that attribute's original ordinal.
In-place/system-catalog details differ again for defaults and indexes. Those
stale-base tuple-mask cases, including PostgreSQL's successful stale no-op,
remain explicit pending obligations under #819/#833; a guarded rejection is not
a passing PostgreSQL parity case. The source selection creates no new durable
format or storage layer and does not qualify new runtime support before its
frozen gate.

SQL catalog own changes have command ownership as well as transaction ownership.
A top-level command pins its catalog owner across internal CTAS steps: PostgreSQL
CTAS counts its new target as absent in its source query, then sees it in the next
command. In an explicit RR extended protocol flow, Bind also pins the command's
own Data overlay and catalog owner. A later own mutation cannot enter an earlier
bound portal; a fresh command can see it. Actual pinned data/catalog portal
oracles establish both distinctions (Jev boundary selection0.99). Savepoint
recovery restores own catalog/cache owners without advancing the retained view.

The current source increment keeps a canonical private publication catalog
through temporary descriptor scopes. SQL catalog command owners compose only
the selected thirteen-map own delta; descriptor owners compose current metadata
with that same delta. Their caches use the overlay version and the pair of Main
and overlay versions respectively. Prepared descriptors are revalidated when an
own catalog stage exists, because the existing single version stamp cannot
represent both versions without collisions.

A private Bind owner carries the command's catalog and, when the compiled plan
requires Data, its COW own-row map. The connection separately admits retained
portal backing; owner clones retain those narrow leases through Close or worker
retirement. Constant queries avoid copying unrelated own payloads. RW RR bound
portals use the existing bounded materialized result path, including suspended
pages; genuinely read-only RR and READ COMMITTED keep their existing direct cursor paths. The current Execute
request supplies cancellation and deadline authority.

Controlled descriptor catalogs remain immutable. Scoped query and session
lookup aliases retain their controlled owner; sequence/default/TRUNCATE and
rollup publishers target the existing mutable transaction stage or Main instead.
Statement mutation forks publish their own metadata changes with their row
changes. Upsert candidate/default evaluation occurs once before the working base
is captured, and an owner wait reuses those prepared inputs. Full transaction
retirement and savepoint restoration discard stale session lookup owners.

At source checkpoint `87753170`, these implementations and their regression
probes remained pending the combined native gate, with the RW admission guard
still in place. The enabled candidate below supersedes that guard status. The
separate existing lifecycle gap for a newly staged SERIAL/identity sequence is
tracked by [#839](https://github.com/cntryl/cassie/issues/839). Published-image and
PostgreSQL 18.6 probes confirm that gap also affects READ COMMITTED; this source
increment does not claim it completed.


### Enabled RW candidate for the combined native gate

The v0.2 profile admits default and explicit READ WRITE
`BEGIN ... REPEATABLE READ`, plus changing READ ONLY to READ WRITE before the
first snapshot. SERIALIZABLE and RR DEFERRABLE remain unsupported. The #819
checkpoint gap for `SET TRANSACTION ISOLATION` is resolved by the
bounded selection recorded on 2026-11-11: support `SET TRANSACTION ISOLATION
LEVEL READ COMMITTED`, `READ UNCOMMITTED` (the selected READ COMMITTED alias),
and `REPEATABLE READ` only in an active explicit transaction before any query
and before any savepoint. A late or subtransaction attempt returns `25001` and
fails the block. An otherwise valid pre-query, pre-savepoint attempt using
`SERIALIZABLE`, DEFERRABLE, `READ ONLY`/`READ WRITE` clauses, or other
`SET TRANSACTION` forms is rejected with `0A000`. Outside an explicit block,
return `25P01` without changing session state; Cassie surfaces
PostgreSQL's `25P01` warning condition as an error because it has no warning or
notice response path. Existing `BEGIN` transaction modes and
`SET transaction_read_only` remain authoritative. Pinned PostgreSQL 18.6 probes
confirm success before a query, `25001` after a query or savepoint, and the
READ UNCOMMITTED spelling; Jev selected the isolation-only form (0.97,
confidence 0.96). The request and oracle receipt are under ignored
`target/v020-evidence/819-set-transaction/`. This bounded syntax selection does
not resolve the remaining #819 acceptance or the separately tracked
catalog-tuple visibility cases under #819/#833.

A legal READ WRITE-to-READ ONLY change only tightens future write permission.
It cannot remove an already captured RW owner's row fence or own-row map.
Before capture, a writable savepoint mode also requires RW authority because
rollback can restore that mode. After capture, that authority remains fixed
through mode changes, savepoint rollback and release. A permanently read-only
view keeps the existing shared transaction owner and direct cursor path without
allocating a Bind frame. Pinned PostgreSQL 18.6 probes confirm the legal mode
changes and savepoint restoration used by the authored regressions.

Statement forks also carry their originating transaction generation. Publication
rejects an old fork after rollback and a new BEGIN, even when the row/catalog
before-images happen to match. This candidate is enabled for executable
qualification, with no passing support or issue-closure claim before the
complete ordered native gate.

The 2026-11-05 integration gate exposed ordinary projector writes consuming the
per-query numeric allowance through this independent shared tracker. The selected
amendment separates its explicit limit from `CASSIE_QUERY_MEMORY_BUDGET_BYTES`
(Jev choice 1.00). Snapshot/catalog ownership still reserves against each query's
budget, and shared history still rejects before publication when its own limit
is exhausted. This preserves writers staged before the first RR capture without
changing the memory limit of a bounded read or allocating unadmitted keys.

### Physical database-family publication authority

The 2026-11-05 source comparison found a Cassie coordination gap: schema
transactions participate in the new schema-family publication guard, while
physical database-family create/drop calls bypassed that same authority. These
lifecycle calls must acquire the existing schema-family guard with current
request controls and latch an unknown native outcome before releasing it. The
bootstrap path resolves the already-created schema family directly and must not
recurse through `ensure_families_ready`.

Jev selected this bounded participation repair at 1.00. It does not establish
engine-wide WAL quiescence or make unobserved native writers/background work
part of Cassie's guard. #1188 retains that broader published-API investigation;
an Internal WAL failure remains an error, never a retry license. Synchronized
native-call exclusion and recovery-quarantine probes qualify the adapter repair
before the concurrent local-storage/restart gate is rerun.

### Bootstrap lifetime and lifecycle replay

The public `bootstrap_families` and `ensure_families_ready` methods initialize
storage at most once per Midge instance. Initial callers serialize through one
bootstrap gate and retry after a failed initialization; initialization publishes
no cached layout on failure. Lifecycle journal replay runs only in that initial
phase, before ordinary database operations can be admitted. A later public
`bootstrap_families` call returns the initialized schema, default-data and temp
handles plus a fresh snapshot of the current database-family registry; it does
not replay journals belonging to live operations. Closing and reopening the
instance begins a new recovery phase. This is an instance-local boundary, not a
Midge WAL quiescence guarantee; #1188 remains open.

Jev selected one-time replay with fresh registry snapshots (choice 0.99). Its
0.53 residual-gap assessment requires synchronized probes of repeated bootstrap
while a database creation is staged, concurrent first admission, and retry after
failed initialization. The selected behavior is supported only after those
probes and the complete ordered gate execute successfully.
An interim adversarial review returned 0.51 for retry after partial initialization
and 0.44 for live create admission before initialization; authored probes exercise
both boundaries. These probes remain unexecuted until the integrated gate.

A physical family already visible under the schema publication gate may be
reused without issuing a create. Once a native create has been submitted, its
error remains an error even if a family with that name becomes visible. Midge
0.5.2 uses `InvalidArgument` for both duplicate and invalid names and supplies
no typed duplicate-create outcome that licenses converting a failed create to
success. An unknown outcome continues to latch recovery; an internal error is
not a retry or success signal. This rule also applies to initial fixed-family
creation.
