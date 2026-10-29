# Product Roadmap

This document owns Cassie's product goal, scope, exclusions, release order, and future work. Current behavior and status live in [Feature Support](feature-support.md); production evidence and blockers live in [Production Readiness](production-readiness.md). The finite local schema/query/pgwire gate is defined in [Local Development Readiness](local-development-readiness.md).

Work is dependency-ordered. A later item does not begin while an earlier correctness or format dependency remains open.

## Product goal and scope

Cassie is a single-node read-model database for event-sourced and CQRS systems.
The event stream remains the source of truth. Projector processes maintain
durable read models through PostgreSQL wire using the documented Cassie SQL
contract; applications query those read models through standard drivers, ORMs,
psql and pgAdmin. PostgreSQL 18.6 is a semantic oracle for selected in-scope
behavior. Cassie does not target complete PostgreSQL server or SQL parity.

The first shippable release, v0.3 Projection Store, is the initial product
milestone: it delivers the write and lifecycle path for maintaining read models.
v0.4 adds the query and retrieval surface. v1.0 qualifies the documented product
for production. Each milestone has its own acceptance scope; completing a later
milestone does not expand Cassie's permanent architectural boundaries.

The bounded extension compatibility surface is part of v0.4: #843 owns the
selected `vector` and `pgcrypto` identities, including the `pgcrypto.digest`
SHA-256 path and extension catalog/lifecycle behavior. #844 owns vector SQL and
retrieval semantics. These are finite SQL compatibility shims, not extension
library loading or a PostgreSQL extension ABI promise.

Every issue and test must serve one of these five product areas:

1. **Projection writes**: idempotent `INSERT ... ON CONFLICT`, `UPDATE` and
   `DELETE`; each replay event batch writes rows in one transaction in the
   owning database family, then commits its duplicate ledger and checkpoint
   together in a second transaction in that same family. Rows may become
   visible before that checkpoint transaction; retrying the same deterministic
   batch must safely reconcile the gap. Current replay does not combine the
   writes into one transaction. Cassie does not use cross-database transactions. `COPY` and
   `TRUNCATE` support rebuilds. There is no separate checkpoint or rebuild API.
2. **Read-model lifecycle**: create, alter and drop schemas, tables, scalar
   indexes, views and materialized views; selected database-scoped DDL publishes
   with row changes in one Midge family; rebuild into a staging table and
   rename-swap; restart and recovery.
3. **Queries**: the explicitly documented `SELECT` behavior, types, functions
   and operators needed by read-model workloads. PostgreSQL compatibility is
   selected per behavior and recorded in the feature-support and compatibility
   contracts; it does not imply support for every PostgreSQL type or function.
4. **Retrieval**: full-text, pgvector-syntax vector and hybrid search.
5. **Client surface**: pgwire, prepared statements, session settings and the
   catalog queries that drivers, ORMs, migration tools and admin clients issue.

The rule is: Cassie implements a documented read-model contract, not PostgreSQL
server parity. A PostgreSQL statement used by a client or migration tool is in
scope only when the roadmap, feature-support record and owning issue define its
behavior. Compatibility shims may accept a client workflow without implementing
the corresponding PostgreSQL server feature.

Permanent exclusions include distributed SQL and cluster coordination, full
PostgreSQL server/extension parity, PL/pgSQL and other stored-program execution,
trigger-based business logic, replication, and a second storage abstraction.
Specific unsupported SQL forms are listed in the feature and compatibility
contracts and must fail deterministically. The former full PostgreSQL 18.6
core-SQL/PL/pgSQL program (#791–#793) was closed as superseded; its remaining
work does not override these product boundaries. Features from that former
program remain active only when a current milestone issue gives them a bounded
read-model purpose. Other parity requests stay outside the release plan unless
a future product decision adds them to the roadmap.

## Milestones

Each milestone is a shippable release. Complete the required local validation
on the exact release commit. GitHub Actions are disabled by user direction, so
hosted evidence is blocked; do not enable or dispatch workflows or claim a
hosted pass.

The dates below are planning targets set on 2026-10-20, not capacity-backed
forecasts. They assume one serial delivery lane and the written exit criteria
only. Recheck progress each Friday. The current issue counts and merged PR rate
do not establish engineering capacity, so keep these dates visible and reforecast
explicitly when a gate slips.

| Target date | Gate | Exit condition | Status |
| --- | --- | --- | --- |
| 2026-10-23 | Replay contract | Prove that the owning database-family row batch can commit before the replay-state transaction, that ledger and checkpoint commit together, and that same-batch retry handles both an aborted replay-state commit and an error response after a successful replay-state commit without losing or double-applying projection state. | Complete locally on 2026-10-20; see [focused evidence](testing/validation.md). Hosted evidence remains blocked while Actions are disabled. |
| 2026-10-30 | v0.3 scope freeze | Map every issue assigned to v0.3 to a written exit criterion; move unrelated compatibility and production-qualification work out of the milestone. Add no new work unless it closes a written exit criterion or resolves a confirmed MVP blocker. | Open; the [issue map](v0.3-scope-map.md) was refreshed against 2026-10-29 GitHub state. After eight deferrals, v0.3 has 16 members (5 open, 11 closed). Selected client/COPY criteria are recorded in #817/#821/#846; #819 and #823 are deferred with their finite boundaries recorded, and #826 now owns the selected lifecycle and rebuild criteria separated from #839. Closed `NOT_PLANNED` #777/#780 do not establish client or wire acceptance. |

The replay recovery proof is complete locally; this does not establish a hosted
pass. If later changes invalidate the proof, keep the v0.3 target visibly at risk
and reforecast at the next Friday review; do not silently weaken the contract.
v0.3 is the MVP qualification target, not approval to replace a production hosted
PostgreSQL service. Production replacement depends on the v1.0 qualification and
workload-specific migration evidence.

The local-development release gate is tracked separately from production
qualification. It requires the selected schema and index lifecycle, relational
query combinations, and pgwire/client workflows in
[Local Development Readiness](local-development-readiness.md). Passing this
gate does not promote broader PostgreSQL compatibility or production support.

The v0.3 scope map records the live membership and a bounded MVP proposal. The
scope-freeze gate remains open until deferred and conditional work is removed
from v0.3, selected client workflows have active owners, and cross-phase or
closed-issue dependencies are reconciled. Local-development slices may proceed
only where their own owner and prerequisite path have been explicitly bounded.

| Milestone | Target date | Ships |
| --- | --- | --- |
| [v0.3 Projection store](https://github.com/cntryl/cassie/milestone/7) | 2026-12-11 | MVP: a projector maintains durable read models on one Cassie node through the selected PostgreSQL-wire write contract. Projection rows, duplicate ledger, and checkpoint use the owning database family; today the row batch and replay-state metadata still commit in separate transactions, and replay retries reconcile a committed row batch when the replay-state commit did not complete. The release also covers schema/object lifecycle, rebuild operations, and named driver/ORM/psql workflows. It excludes cross-database transactions and full PostgreSQL parity. |
| [v0.4 Query and retrieval](https://github.com/cntryl/cassie/milestone/8) | 2027-04-09 | The documented read-side contract for read-model workloads: selected relational SQL, type and function families, bounded extension compatibility in #843, plus full-text, vector and hybrid retrieval. Each capability is bounded by Feature Support and its live issue criteria; “complete SELECT” means the documented Cassie profile, not PostgreSQL-wide syntax or semantics. |
| [v1.0 Production release](https://github.com/cntryl/cassie/milestone/9) | 2027-08-27 | Production qualification of the projection store and documented query surface: representative single-node resource and concurrency evidence, cross-path invariants, restart/recovery and rebuild, operational thresholds and one release ledger. Qualification does not expand product scope. |

Requests for additional type families enter a release milestone only after the
current Type Contract selects their logical identity, SQL and wire semantics,
and durable representation where required. Already implemented native
families are part of the current type inventory; unselected requests such as
range, XML, geometric and money types remain proposals outside v0.4 until
selected for a documented read-model use case.

Failing tests are the implementation backlog: implement the behavior; never
ignore, skip or weaken a test. A test for an out-of-scope capability is
removed, not implemented.

## Production Evidence

- define named disk-backed deployment profiles;
- retain complete same-commit benchmark manifests at representative scale and concurrency;
- establish latency, capacity, cancellation, and recovery objectives per profile;
- exercise backup, restore, rebuild, repair, and failure-injection runbooks;
- promote feature families only when implementation, compatibility, performance, and readiness owners agree.

## Typed Column Execution Target

The finite [Query Engine Target 1](query-engine-target.md) and complete
[ownership ledger](query-engine-invariant-ownership.md) define the milestone gates,
exact child blockers, semantic oracles and promotion boundaries. Existing Stable
CBM2 codecs and selected column acceleration do not establish a common typed
relational pipeline; that pipeline remains Planned.

1. Contracts and correctness: retain the merged [#748](https://github.com/cntryl/cassie/issues/748)/[#749](https://github.com/cntryl/cassie/issues/749)/[#750](https://github.com/cntryl/cassie/issues/750) repairs, [#432](https://github.com/cntryl/cassie/issues/432) Boolean contract and [#751](https://github.com/cntryl/cassie/issues/751)/[#754](https://github.com/cntryl/cassie/issues/754) foundation;
   retain the merged [finite type/codec contract](type-contract.md) in [#752](https://github.com/cntryl/cassie/issues/752) and the [private batch/view contract](typed-batch-contract.md) in [#753](https://github.com/cntryl/cassie/issues/753).
   [PR #790](https://github.com/cntryl/cassie/pull/790), commit
   `9ff42bc05c374f478a12a29caf7537268215e665`, completed #753 and the selected
   #755 bounded wire-cycle [contract](pgwire-transaction-contract.md), including validation and merge/closure readback.
2. Typed batches and streaming: [#756](https://github.com/cntryl/cassie/issues/756) followed [#753](https://github.com/cntryl/cassie/issues/753)/[#749](https://github.com/cntryl/cassie/issues/749)/[#432](https://github.com/cntryl/cassie/issues/432)/[#755](https://github.com/cntryl/cassie/issues/755) and delivered the first selected
   scan/filter/projection slice with bounded scalar fallback and admitted row/wire
   conversion. [#757](https://github.com/cntryl/cassie/issues/757) followed [#755](https://github.com/cntryl/cassie/issues/755)/[#752](https://github.com/cntryl/cassie/issues/752) and qualified selected portal ownership.
   [PR #851](https://github.com/cntryl/cassie/pull/851), commit
   `4a09c41a3670dad3a4c4c2b7ef7583327c37ab6b`, records both finite closures and
   [executed witnesses](typed-pipeline-acceptance.md). Milestones 1 and 2 closed
   on 2026-10-11; complete relational integration and promotion remain separate.
3. Relational kernels: [#758](https://github.com/cntryl/cassie/issues/758) aggregates and [#759](https://github.com/cntryl/cassie/issues/759) joins follow [#756](https://github.com/cntryl/cassie/issues/756); [#760](https://github.com/cntryl/cassie/issues/760) ordering,
   sets and windows follows both; [#761](https://github.com/cntryl/cassie/issues/761) qualifies completed relational paths;
   [#762](https://github.com/cntryl/cassie/issues/762) then adds the selected bounded scalar expression subquery surface.
4. Storage and retrieval qualification: [#763](https://github.com/cntryl/cassie/issues/763)–[#776](https://github.com/cntryl/cassie/issues/776) follow their individual ledger
   dependencies for visibility, constraints, cache/auth, columns/codecs, derived
   state, providers, ANN/text/hybrid/graph/time-series and portable kernel laws.
   Retain #763's completed finite Data-family statement-view and immutable-overlay
   prerequisite from [PR #871](https://github.com/cntryl/cassie/pull/871), commit
   `ce72a23f2d0a3cdb9a811107aec7629151fcd319`; full #819/#833 isolation and
   transactional-catalog acceptance remains within the projection-store and
   production milestones under the owning issue criteria; the former parity
   program is closed and is not a dependency.
5. Compatibility and release evidence: [#777](https://github.com/cntryl/cassie/issues/777)–[#780](https://github.com/cntryl/cassie/issues/780) qualify the completed selected
   client/wire/control/cross-path subset; [#8](https://github.com/cntryl/cassie/issues/8) owns native operational objectives
   and repeated evidence, [#29](https://github.com/cntryl/cassie/issues/29) owns local diagnostic promotion after [#8](https://github.com/cntryl/cassie/issues/8), and [#781](https://github.com/cntryl/cassie/issues/781)
   consolidates one immutable release ledger.

No public batch API, persistent encoding, migration, new numeric/temporal/extension
ABI, disk spilling or distributed execution is selected by these tracking stages.
Portable kernels are the oracle; optional SIMD requires measured benefit and
explicit feature/alignment/tail/mask semantics. A later stage waits for its named
dependencies; mapped source/tests and issue closure do not promote support.


## Query Depth

- Promote Experimental query families only through their documented promotion criteria.
- Use the completed [Query Promotion Evidence](query-promotion-evidence.md) inventory when
  reviewing each Experimental family; retain its seeded differential and metamorphic evidence
  instead of duplicating family-specific corruption, cancellation, or resource-control suites.
- Keep feedback-informed planning and checkpointed switching observable and opt-in until representative workloads justify default enablement.

## Compatibility Depth

- Expand opt-in sqlx, Diesel, Prisma, SQLAlchemy, psql, and pgAdmin workflow probes without implying full PostgreSQL parity.
- Add catalog rows or protocol behavior only for documented read-model client workflows.
- Keep unsupported isolation, extension, distributed, trigger, and business-procedure behavior explicit.

## Operational Scale

- Improve local capacity, health, drain-state, and projection diagnostics that external operators can consume.
- Keep all routing decisions, placement, failover, data movement, and fleet coordination external.
- Distributed SQL, cluster management, membership, replication, consensus, sharding or rebalancing, cross-node transactions, distributed planning, remote query forwarding, and automatic cross-node repair are permanent non-goals, not future roadmap items.
- Keep Midge as the only persistence layer and dependency owner for durability and recovery mechanics.
