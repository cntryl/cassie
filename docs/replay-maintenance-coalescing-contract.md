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
