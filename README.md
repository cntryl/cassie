# Cassie

Cassie is a purpose-built read-model database for CQRS and event-sourced systems. It is designed for fast, predictable, and operationally simple queries over projection data derived from event streams.

The event stream is the source of truth. Cassie is the read model. That distinction shapes every architectural decision.

## Design Principles

### Single-node first

Cassie prioritizes exceptional single-node performance over distributed complexity. Every feature, optimization, and storage decision should make a single Cassie node faster, simpler, or more predictable.

### Operational scale over distributed SQL

Cassie scales by adding more nodes, not by distributing individual queries. Its focus is on workload isolation, projection ownership, tenant routing, partition assignment, and horizontal expansion of independent read nodes.

### Purpose-built for read models

Cassie is optimized for the query patterns that matter in real-world read models, including primary-key lookups, secondary-index lookups, time-range queries, aggregations, reporting workloads, full-text search, vector search, and hybrid search.

### Performance is a feature

Performance is not an afterthought. Query patterns should be benchmarked, measured, and continuously validated against real workloads and explicit latency targets.

### Event-sourcing native

Cassie assumes an event-sourced architecture. Data is projection data, projections can be rebuilt, and recovery is achieved through replay, snapshots, and rebuilds rather than complex database recovery procedures.

### Simplicity wins

Simplicity is a competitive advantage. A system that is easy to understand, operate, debug, and evolve will outperform a more sophisticated one that constantly demands operational attention.

## Product Priorities

### Tier 1: Core read-model database

- Performance benchmarking
- Query planner maturity
- Secondary-index completeness
- Aggregation support
- Efficient range queries

### Tier 2: Production read models

- Materialized views
- Time-series capabilities
- Window functions
- Read-model-oriented joins

### Tier 3: Intelligent retrieval

- Full-text search
- Vector search
- Hybrid search

### Tier 4: Operational scale

- Node routing
- Projection ownership
- Snapshot and restore
- Replay and rebuild tooling
- Health monitoring
- Capacity management

## Non-goals

Cassie is not pursuing full PostgreSQL feature parity, distributed SQL execution, distributed transactions, multi-node query planning, stored procedures, trigger-based business logic, OLTP optimization, or consensus-heavy clustering. Those concerns belong elsewhere in the architecture.

## Vision

Cassie aims to be the simplest, fastest, and most predictable database for building read models. A Cassie deployment should be easy to understand, easy to operate, easy to scale, and capable of serving demanding query workloads with consistent performance.

The event stream owns truth. Cassie owns query performance. Everything else is secondary.