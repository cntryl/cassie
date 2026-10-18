# Product Roadmap

This document contains future work only. Current behavior and status live in [Feature Support](feature-support.md); readiness evidence and blockers live in [Production Readiness](production-readiness.md).

Work is dependency-ordered. A later item does not begin while an earlier correctness or format dependency remains open.

## Product scope: a query engine for read models

Cassie powers read models of event-sourced and CQRS systems. The event stream
is the source of truth; projector processes write read-model tables over the
PostgreSQL wire protocol with plain SQL, and applications query them through
standard drivers, ORMs, psql and pgAdmin. Every issue and test is judged by
whether it serves one of these five areas:

1. **Projection writes**: idempotent `INSERT ... ON CONFLICT`, `UPDATE` and
   `DELETE`; an event batch and its checkpoint row committed in one
   transaction; `COPY` and `TRUNCATE` for rebuilds. There is no separate
   checkpoint or rebuild API.
2. **Read-model lifecycle**: create, alter and drop tables, indexes, views and
   materialized views; rebuild into a staging table and rename-swap; restart
   and recovery.
3. **Queries**: complete PostgreSQL `SELECT` semantics over every PostgreSQL
   data type a read model can store (numeric, temporal, UUID, JSON, arrays,
   enums, geometry, money, XML, ranges, network, bytea, composite and domain)
   and their functions and operators.
4. **Retrieval**: full-text, pgvector-syntax vector and hybrid search.
5. **Client surface**: pgwire, prepared statements, session settings and the
   catalog queries that drivers, ORMs, migration tools and admin clients issue.

The rule is: data types and query semantics are in scope; server
programmability and administration are not. Statements that migration tools
and clients issue automatically (advisory locks, `COMMENT ON`, `GRANT`/roles,
`FOR UPDATE`, `ANALYZE`/`VACUUM`, `CREATE EXTENSION`, size and activity
functions, temporary tables) are supported at the documented compatibility
level recorded on their owning issue, without full server semantics.

Out of scope, rejected with a deterministic unsupported-feature error:
PL/pgSQL, `DO` blocks and SQL-language routines; triggers and event triggers;
rewrite rules; row-level security and privilege enforcement beyond `root`;
LISTEN/NOTIFY; large objects; table inheritance; declarative partitioning;
user-defined operators, operator families, casts, conversions and aggregates;
extended statistics; two-phase transactions; exclusion and temporal
constraints; data-modifying CTEs; text-search configuration DDL. The prior
"complete PostgreSQL 18.6 core SQL and full PL/pgSQL" target (#791–#793) is
superseded.

## Milestones

Each milestone is a shippable release; a green backend CI run and a passing
full test suite on the release commit block every release.

| Milestone | Ships |
| --- | --- |
| [v0.3 Projection store](https://github.com/cntryl/cassie/milestone/7) | A projector maintains read models through any PostgreSQL driver: projection writes, read-model types, table/index/view lifecycle, rebuild via `TRUNCATE`/`COPY`/rename-swap, and driver/ORM/migration-tool/psql connectivity. |
| [v0.4 Query and retrieval](https://github.com/cntryl/cassie/milestone/8) | The read side: subqueries, CTEs, aggregates, windows, pattern matching, JSON, arrays, temporal, numeric and the remaining data-type families, plus full-text, pgvector and hybrid retrieval. |
| [v1.0 Production release](https://github.com/cntryl/cassie/milestone/9) | The projection store and query surface qualified for production: storage and retrieval qualification, cross-path invariants, restart/recovery and rebuild evidence, operational thresholds and a release ledger. |

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
   transactional-catalog acceptance remains with the PostgreSQL parity program.
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
