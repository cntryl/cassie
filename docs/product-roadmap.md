# Product Roadmap

This document contains future work only. Current behavior and status live in [Feature Support](feature-support.md); readiness evidence and blockers live in [Production Readiness](production-readiness.md).

Work is dependency-ordered. A later item does not begin while an earlier correctness or format dependency remains open.

## Query Baseline Closure

1. Close graph, time-series, and column-batch differential, codec, access-path, cancellation, and memory-bound contracts.
2. Enable feedback-informed planning and checkpointed operator switching by default; prove configured parallel execution under shared permits.
3. Add cross-family property, metamorphic, seeded differential, pagination, corruption, cancellation, and source-boundary suites.

## Production Evidence

After the query baseline closes:

- define named disk-backed deployment profiles;
- retain complete same-commit benchmark manifests at representative scale and concurrency;
- establish latency, capacity, cancellation, and recovery objectives per profile;
- exercise backup, restore, rebuild, repair, and failure-injection runbooks;
- promote feature families only when implementation, compatibility, performance, and readiness owners agree.

## Compatibility Depth

- Expand opt-in sqlx, Diesel, Prisma, SQLAlchemy, psql, and pgAdmin workflow probes without implying full PostgreSQL parity.
- Add catalog rows or protocol behavior only for documented read-model client workflows.
- Keep unsupported isolation, extension, distributed, trigger, and business-procedure behavior explicit.

## Operational Scale

- Improve external node routing, drain, move, projection ownership, and capacity signals while keeping nodes independent.
- Keep replication, consensus, cross-node transactions, distributed planning, and automatic cross-node repair outside Cassie.
- Keep Midge as the only persistence layer and dependency owner for durability and recovery mechanics.
