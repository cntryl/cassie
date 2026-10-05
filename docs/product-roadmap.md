# Product Roadmap

This document contains future work only. Current behavior and status live in [Feature Support](feature-support.md); readiness evidence and blockers live in [Production Readiness](production-readiness.md).

Work is dependency-ordered. A later item does not begin while an earlier correctness or format dependency remains open.

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

1. Contracts and correctness: retain the merged [#748](https://github.com/cntryl/cassie/issues/748)/[#749](https://github.com/cntryl/cassie/issues/749)/[#750](https://github.com/cntryl/cassie/issues/750) repairs and the merged [#432](https://github.com/cntryl/cassie/issues/432) Boolean contract; reconcile [#751](https://github.com/cntryl/cassie/issues/751)/[#754](https://github.com/cntryl/cassie/issues/754);
   pin current types/codecs in [#752](https://github.com/cntryl/cassie/issues/752) and the private batch/view contract in [#753](https://github.com/cntryl/cassie/issues/753).
   [#755](https://github.com/cntryl/cassie/issues/755) implements and qualifies the user-selected bounded wire-cycle visibility
   contract; selecting its profile does not complete that runtime dependency.
2. Typed batches and streaming: [#756](https://github.com/cntryl/cassie/issues/756) follows [#753](https://github.com/cntryl/cassie/issues/753)/[#749](https://github.com/cntryl/cassie/issues/749)/[#432](https://github.com/cntryl/cassie/issues/432)/[#755](https://github.com/cntryl/cassie/issues/755) and carries
   scan/filter/projection through the selected type/kernel matrix, with bounded
   scalar fallback and explicit row/wire conversion. [#757](https://github.com/cntryl/cassie/issues/757) follows [#755](https://github.com/cntryl/cassie/issues/755)/[#752](https://github.com/cntryl/cassie/issues/752) and
   qualifies portal ownership.
3. Relational kernels: [#758](https://github.com/cntryl/cassie/issues/758) aggregates and [#759](https://github.com/cntryl/cassie/issues/759) joins follow [#756](https://github.com/cntryl/cassie/issues/756); [#760](https://github.com/cntryl/cassie/issues/760) ordering,
   sets and windows follows both; [#761](https://github.com/cntryl/cassie/issues/761) qualifies completed relational paths;
   [#762](https://github.com/cntryl/cassie/issues/762) then adds the selected bounded scalar expression subquery surface.
4. Storage and retrieval qualification: [#763](https://github.com/cntryl/cassie/issues/763)–[#776](https://github.com/cntryl/cassie/issues/776) follow their individual ledger
   dependencies for visibility, constraints, cache/auth, columns/codecs, derived
   state, providers, ANN/text/hybrid/graph/time-series and portable kernel laws.
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
