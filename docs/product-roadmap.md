# Product Roadmap

This document contains future work only. Current behavior and status live in [Feature Support](feature-support.md); readiness evidence and blockers live in [Production Readiness](production-readiness.md).

Work is dependency-ordered. A later item does not begin while an earlier correctness or format dependency remains open.

## Future Column Codecs

- ALP — Planned. Add an independently specified and promoted floating-point codec only after
  its exactness, deterministic byte representation, fallback, and benchmark contracts exist.
- FSST — Planned. Add an independently specified and promoted string codec only after its
  bounded dictionary format, corruption handling, cross-architecture fixtures, and query
  evidence exist.

CBM2 codec selection is automatic and is not a SQL or configuration surface. Cassie does not
add LZ4 or Zstd above Midge's generic storage compression; any future codec must remain a
Cassie-owned derived format with authoritative-row fallback.

## Production Evidence

- define named disk-backed deployment profiles;
- retain complete same-commit benchmark manifests at representative scale and concurrency;
- establish latency, capacity, cancellation, and recovery objectives per profile;
- exercise backup, restore, rebuild, repair, and failure-injection runbooks;
- promote feature families only when implementation, compatibility, performance, and readiness owners agree.

## Query Depth

- Promote Experimental query families only through their documented promotion criteria.
- Expand cross-family property, metamorphic, seeded differential, pagination, corruption, cancellation, and source-boundary suites where promotion evidence identifies a gap.
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
