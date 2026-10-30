# Product Roadmap

This document owns Cassie's product goal, scope, exclusions, release order, and future work. The unreleased 0.1.0 starting point is recorded in [Release Baseline](release-baseline.md). Current behavior and status live in [Feature Support](feature-support.md); production evidence and blockers live in [Production Readiness](production-readiness.md). The finite local schema/query/pgwire gate is defined in [Local Development Readiness](local-development-readiness.md).

Work is dependency-ordered. A later item does not begin while an earlier correctness or format dependency remains open.

## Product goal and scope

Cassie is a single-node read-model database for event-sourced and CQRS systems.
The event stream remains the source of truth. Projector processes maintain
durable read models through PostgreSQL wire using the documented Cassie SQL
contract; applications query those read models through standard drivers, ORMs,
psql and pgAdmin. PostgreSQL 18.6 is a semantic oracle for selected in-scope
behavior. Cassie does not target complete PostgreSQL server or SQL parity.

Cassie has never been released. The first planned release, v0.1.0 Projection
Store, is the initial MVP: it delivers the write and lifecycle path for
maintaining read models. The earlier 0.2.0 package value and former v0.3
Projection Store and v0.4 Query labels were unreleased development planning. The
active feature releases are v0.1.0 Projection Store, v0.2.0 Query and
Retrieval, and v0.3.0 Authoritative Columnar Tables. Named enum support in #827
is deferred to a new v0.4.0 milestone after those active releases; it is not
part of the current delivery order. Production readiness is a separate
qualification gate after the feature releases, not a feature milestone. Each
release has its own acceptance scope; completing a later release does not
expand Cassie's permanent architectural boundaries.

The bounded extension compatibility surface is part of v0.2.0: #843 owns the
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
5. **Client surface**: pgwire, prepared statements, session settings, selected
   projector/reader access controls, and the catalog queries that drivers,
   ORMs, migration tools and admin clients issue.

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

## Planning gates

Each feature milestone is a shippable release. Complete the required local
validation on the exact release commit. The repository Actions permission readback
reports `enabled: true` as of 2026-10-30, but `AGENTS.md` still prohibits enabling
or dispatching workflows until the user explicitly changes that direction. Do not
dispatch; report required backend and release workflow evidence as blocked.

The dates below are planning targets set on 2026-10-20, not capacity-backed
forecasts. They assume one serial delivery lane and the written exit criteria
only. Recheck progress each Friday. The current issue counts and merged PR rate
do not establish engineering capacity, so keep these dates visible and reforecast
explicitly when a gate slips.

| Target date | Gate | Exit condition | Status |
| --- | --- | --- | --- |
| 2026-10-23 | Replay contract | Prove that the owning database-family row batch can commit before the replay-state transaction, that ledger and checkpoint commit together, and that same-batch retry handles both an aborted replay-state commit and an error response after a successful replay-state commit without losing or double-applying projection state. | Complete locally on 2026-10-20; see [focused evidence](testing/validation.md). Hosted evidence remains blocked while Actions are disabled. |
| 2026-10-30 | Initial MVP scope freeze (pre-release planning label) | Map every issue assigned to the projection-store milestone to a written exit criterion; move unrelated compatibility and production-qualification work out of the milestone. Add no new work unless it closes a written exit criterion or resolves a confirmed MVP blocker. | Complete; the selected client and wire scopes (#777/#780), COPY ingress (#846), catalog/TypeInfo work (#817), table/rebuild lifecycle (#826), and scalar-index lifecycle (#842) are recorded against their merged evidence. The selected MVP is the v0.1.0 starting point; broader query/client behavior is assigned to v0.2.0 where it supports a named read-model workflow. The exact release commit still needs its own gate. Actions permission reports enabled, but the no-dispatch instruction remains active; no backend workflow result is claimed. |

The replay recovery proof is complete locally; this does not establish a hosted
pass. If later changes invalidate the proof, keep the v0.1.0 target visibly at
risk and reforecast at the next Friday review; do not silently weaken the
contract. v0.1.0 is the MVP feature release, not approval to replace a
production hosted PostgreSQL service. Production replacement requires the
separate production-readiness gate and workload-specific migration evidence.

The local-development release gate is tracked separately from production
qualification. It requires the selected schema and index lifecycle, relational
query combinations, and pgwire/client workflows in
[Local Development Readiness](local-development-readiness.md). Passing this
gate does not promote broader PostgreSQL compatibility or production support.

The [v0.1.0 scope map](v0.1-scope-map.md) records the bounded MVP contract.
The current open issue-to-release assignment is maintained in
[Release Milestones](release-milestones.md); every open issue must either
belong to an active feature release, a named deferred future milestone, or be
reworked and closed with a recorded disposition. The only current deferred
feature is #827 in v0.4.0.

## Feature releases

| Milestone | Target date | Ships |
| --- | --- | --- |
| [v0.1.0 Projection Store](https://github.com/cntryl/cassie/milestone/7) | 2026-12-11 | MVP: a projector maintains durable read models on one Cassie node through the selected PostgreSQL-wire write contract. Projection rows, duplicate ledger, and checkpoint use the owning database family; today the row batch and replay-state metadata still commit in separate transactions, and replay retries reconcile a committed row batch when the replay-state commit did not complete. The release also covers schema/object lifecycle, rebuild operations, and named driver/ORM/psql workflows. It excludes cross-database transactions and full PostgreSQL parity. |
| [v0.2.0 Query and Retrieval](https://github.com/cntryl/cassie/milestone/8) | 2027-04-09 | The finite read-side contract for read-model workloads: selected relational SQL, transaction/query semantics, types and functions, catalog/client support, role-based projector and reader access, and full-text, vector and hybrid retrieval. Each capability is bounded by Feature Support and its live issue criteria; this is not PostgreSQL-wide syntax or semantics. |
| [v0.3.0 Authoritative Columnar Tables](https://github.com/cntryl/cassie/milestone/10) | TBD | Add an opt-in per-table column-authoritative layout alongside row-authoritative tables, with a versioned contract, durable recovery, mixed-mode queries and writes, constraints, derived indexes, and explicit conversion/image behavior. Midge remains the only storage layer. |
| [v0.4.0 Named Enum Types](https://github.com/cntryl/cassie/milestone/11) (deferred) | TBD | After v0.1.0–v0.3.0 complete, implement only the named ENUM behavior in #827. First define logical identity, SQL/wire semantics, label ordering, schema/dependency lifecycle, restart, and durable representation in the Type Contract. This does not add general PostgreSQL type or catalog parity. |

Production readiness is tracked separately in [Production Readiness](production-readiness.md) and the release checklist. It covers representative-scale resources, concurrency, recovery, operations, and release evidence; it does not create another feature milestone.

Requests for additional type families enter a release milestone only after the
current Type Contract selects their logical identity, SQL and wire semantics,
and durable representation where required. Already implemented native
families are part of the current type inventory; unselected requests such as
range, XML, geometric and money types remain proposals outside v0.2 until
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
CBM1 codecs and selected column acceleration do not establish a common typed
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
   `ce72a23f2d0a3cdb9a811107aec7629151fcd319`. Selected same-family
   transactional-catalog runtime is part of the 0.1.0 projection-store
   baseline. The finite atomic-publication behavior required by v0.2.0 table
   forms remains under #833, consumed by #826 and #839. Higher isolation and
   wider conflict-wait behavior is assigned to #819 in v0.2.0. The former
   parity program is closed and is not a dependency.
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
