# Replay maintenance coalescing: proof gate

Issue [#1277](https://github.com/cntryl/cassie/issues/1277) investigates a
bounded optimization of replay's postcommit maintenance. This document records
an implementation candidate and its proof obligations; it does not assert that
coalescing is implemented or qualified. Until the obligations below are met,
existing maintenance remains authoritative.

## Candidate and scope

For a standalone native replay with no observed column index, retain a Midge
Data-family read-write transaction beginning after the primary replay commit and
before either maintenance attempt. Capture the generation and the existing
column-batch and projection-hash debt bytes from that transaction's snapshot.
Run both existing artifact attempts and their failure diagnostics. In the
retained transaction, assert the captured generation and, independently for each
successful attempt, that artifact's captured debt bytes; delete only successful
artifacts whose debt existed at capture. Commit those deletions together with
existing synchronous maintenance options. A failed artifact's diagnostic
write must not be asserted or deleted by that transaction.

The no-index observation is an optimization route, not authority to omit debt,
column maintenance, or index publication. An indexed replay, a transactional
DDL storage scope, or an unproved route retains existing maintenance. If the
pre-work transaction cannot be acquired or admitted, select existing maintenance
without failing an already-committed replay. A known assertion conflict leaves
unresolved debt intact; it does not authorize a blind delete retry. Native
outcome-unknown continues to use the existing publication recovery fence and must
not be described as a rollback. Whether its handling preserves all current replay
acknowledgement semantics must be proven before implementation.

## Facts and unresolved proof

Published Midge 0.5.2 assertions compare the transaction's frozen snapshot and
reject a later sequence change on the asserted key, including change-and-revert.
This can fence generation and debt updates after the retained transaction starts,
including writes through an escaped native transaction. A late cleanup
transaction cannot detect earlier change-and-revert. Column incremental
maintenance can rewrite unchanged generation bytes as its own conflict fence;
an assertion spanning that write would conflict, so indexed cases are not
eligible for this candidate. A staged `AdapterTransaction` copies mutations to an
overlay and drops the native transaction; its native assertions do not survive
that handoff.

The candidate requires source review and deterministic evidence for all of the
following before runtime changes are selected:

- Normal and pending index creation, deletion and recovery coordinate with the
  replay collection gate through both the routing and actual column scan; a
  route observation never substitutes for publication validation. A raw writer
  that changes reserved index keys without generation or debt updates is a
  separate interop boundary: define the supported contract and preserve existing
  native read-write interop rather than assuming every raw key mutation is safe.
- Original generation and debt bytes represent the committed replay, and the
  cleanup assertion handles same-value rewrites, ABA, generation changes,
  collection drop/recreate, both partial-failure orders, public debt retry, and
  indexed work discovered after routing. Conflict or unknown leaves the
  committed replay and repair semantics intact.
- The retained native snapshot pins Midge resources until commit or drop. Bound
  or otherwise justify its lifetime across actual hash work, including stale
  metadata, cancellation, write stall, memory admission and panic paths. Existing
  eager hash work can scan the collection; do not infer a lifetime bound solely
  from a metadata row count. Release the owner on every exit.
- Delayed successful debt cleanup can extend reader fallback. Verify the
  observable state, index lifecycle and recovery across interruption and local
  restart, including strict and buffered replay. Do not claim a power-loss or
  cloud-loss result from a local process-death test.
- Measure full replay acknowledgement and foreground/native commit attempts with
  matched indexed and unindexed fixtures; label engine-wide WAL counters and
  background work. A saved cleanup transaction does not itself prove a saved
  filesystem sync or latency improvement.

The preceding points are acceptance work, not executable evidence. This proposal
does not change the published Midge dependency or on-disk debt format, downgrade
WAL durability, or alter strict replay.

### Snapshot-owner limitation

Midge's transaction owns a snapshot pin, and its timeout handling warns but
retains that pin until the transaction closes; it does not provide a public
per-transaction expiry that would release it while synchronous hash maintenance
is still running. Cassie's hash path has no cancellation hook for such an owner.
An elapsed-time check after a blocking hash call is not a bound on retention.
The candidate therefore remains blocked until a scoped, reviewed owner-lifetime
mechanism or an exact conservative route is demonstrated. Stale-row-count
routing alone does not establish the bound. Any new persistent revision or
staged-assertion support requires its own documented contract and migration or
lifecycle proof; no unpublished Midge capability is assumed.

## Selected outcome for #1277 (2026-11-15)

Current post-write maintenance is retained unchanged. Every candidate above
that removes or coalesces a synchronous maintenance commit is still blocked by
an unresolved proof obligation recorded in this document and in
[#1277](https://github.com/cntryl/cassie/issues/1277): debt-attempt identity
and ABA, the unbounded snapshot pin of a retained transaction, raw reserved-key
writers, and staged-assertion propagation. No debt is skipped, deferred or
cleared differently, strict replay is unchanged, and no latency or sync saving
is claimed.

What is implemented is attribution, the issue's first acceptance criterion.
With the `replay-benchmark-observation` feature, each caller-thread native
commit attempt is attributed to exactly one source: the primary commit, column
batch maintenance, projection hash maintenance or time-series maintenance, and
local-sync or cloud-strict attempts inside a maintenance scope are counted as
maintenance barriers. The scope is entered inside
`complete_column_batch_maintenance`, `complete_projection_hash_maintenance` and
`rebuild_time_series_indexes_for_collection`, so the replay API, SQL commit and
repair callers are all attributed; the outermost maintenance scope wins. Without
the feature the scope is a no-op. The Tier 3 replay durability driver reports
these per batch for indexed and unindexed shapes.

Attribution limits: these are call attempts on the observing thread, not
physical syncs; background threads, staged overlays and suppressed commits are
excluded; one WAL sync can satisfy several attempts. Use the counts to decide
which maintenance path to prove first, not as a measured saving.

The remaining acceptance criteria (a reviewed debt, generation and repair
contract, deterministic index-lifecycle and restart tests for a changed
maintenance path, and matched latency comparison) apply to a future
optimization and stay open on #1277.

## Selected reduction: buffered intermediate hash rebuild commits (2026-11-15)

Supersedes "current post-write maintenance is retained unchanged" for the eager
projection hash rebuild only. The candidates above stay blocked; this one does
not depend on debt identity, a retained transaction or a format change.

**Change.** `rebuild_projection_hashes` commits, in order: delete the root,
delete obsolete row and range hash keys, write row-hash chunks, write
range-hash chunks, write the root. The first four now use the buffered policy;
root publication, the hash metadata update, the failure record and the debt
clear stay synchronous. `WritePolicy::buffered()` returns the buffered option
only for local storage. Cloud-background and cloud-strict return exactly what
`sync()` returns, so cloud behaviour is unchanged.

**Why it is safe.**

- This relies only on Midge's published external contract, not on engine
  internals. `cntryl-midge` 0.5.2
  `docs/user-guides/transaction-durability-contract.md` states that successful
  writes publish in admitted sequence order; that every engine tracks monotonic
  frontiers and "a write at sequence `s` has crossed a frontier once the
  frontier is at or above `s`"; that `sync()` crosses the local durable frontier
  at return; and that a `buffered()` write is kept across a process crash "only
  if the later fsync ran". A synchronous commit at a later sequence therefore
  places every earlier buffered commit at or below the local durable frontier.
  The engine source (`runtime/frontiers.rs`, sealed-segment sync in
  `wal/fs/writer_io.rs`) is consistent with that contract and was read only to
  check it; Cassie does not depend on anything the contract does not state.
- The durable state when the function returns is therefore the same as before:
  the synchronous root commit is the durability barrier for the whole rebuild.
- A crash before the root commit leaves a durable prefix of the same commit
  sequence with no root. Every such prefix was already reachable when all five
  commits were synchronous, so recovery and reader fallback see no new state.
- In-process visibility between commits is unchanged; buffered commits are
  applied locally. The collection write gate, generation check, durable
  maintenance debt and failure recording are untouched.
- Strict replay is unaffected: its primary commit stays synchronous and the
  acknowledgement still follows the synchronous root and debt-clear commits.

Jev selected this candidate (0.94) and flagged the prefix-durability assumption
(0.65); the published durability contract quoted above is what resolves it.
Power loss cannot be exercised by a process-level test, so the ordering claim
rests on that published contract, not on a test. The same contract bounds what
this change may assume: local disk loss still loses local-only data, and
nothing here applies to cloud policies.

**Evidence.** Reopen tests for a completed and an interrupted rebuild; with the
`replay-benchmark-observation` feature, the local durable frontier equals the
current sequence when the rebuild returns and its commits are observed as
buffered followed by synchronous root publication. The existing interrupted
rebuild, process-death replay and strict or buffered replay suites continue to
apply.

**Correctness coverage added (#1277).** Under both the strict and the
replayable replay profile: an unindexed projection publishes current hashes; a
scalar index that exists before replay, is created between batches, or is
dropped between batches still serves the replayed rows; an interrupted hash
refresh leaves durable debt and readable rows, and `retry_maintenance_debt`
restores current hashes; and after an interrupted refresh a restart on local
storage repairs the hashes. These are process-level tests; they do not exercise
power loss.

**Not changed.** Column batch maintenance, the stale-marking path for large
collections, time-series maintenance and both debt clears keep their existing
synchronous commits.
