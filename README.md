# Cassie

Cassie is a purpose-built read-model database for CQRS and event-sourced systems. It is designed for fast, predictable, and operationally simple queries over projection data derived from event streams.

The event stream is the source of truth. Cassie is the read model. That distinction shapes every architectural decision.

## POC Quickstart

Run a local embedded read-model proof of concept:

```sh
cargo run --locked --example poc_read_model
```

The example creates a projection-style table, inserts rows, adds a scalar index, runs filtered and
aggregate SQL, prints EXPLAIN output, and reports health/metrics. It uses Midge's in-memory fallback
and is a POC path, not a production deployment recipe.

## Runtime Configuration

Runtime configuration is driven by `CASSIE_*` environment variables.

| Variable | Default | Notes |
| --- | --- | --- |
| `CASSIE_PGWIRE_LISTEN` | `127.0.0.1:5432` | PostgreSQL wire listener address |
| `CASSIE_REST_LISTEN` | `127.0.0.1:8080` | REST listener address |
| `CASSIE_ADMIN_UI_DIR` | `./ui/dist` | Built admin UI asset directory served at `/` (and any unmatched non-API path, for client-side routing) plus `/assets/*`; all resource APIs live under `/api/v1/*`, with `/health`, `/liveness`, `/targetz`, and `/metrics` reserved as unauthenticated probe endpoints |
| `CASSIE_PGWIRE_MAX_CONNECTIONS` | `256` | Pgwire admission cap, clamped to at least `1` |
| `CASSIE_REST_MAX_CONNECTIONS` | `512` | REST admission cap, clamped to at least `1` |
| `CASSIE_MAX_QUERY_WORKERS` | `64` | Local query-worker admission cap, clamped to at least `1`; exhausted queries return SQLSTATE `53300` |
| `CASSIE_EMBEDDINGS_PROVIDER` | `disabled` | Supported values are `disabled`, `openai`, `openai_compatible`, `tei`, `ollama`, `voyage`, `cohere`, and `local` |

Provider-specific embedding config stays under the matching prefix:

- `CASSIE_VOYAGE_API_KEY`, `CASSIE_VOYAGE_MODEL`, `CASSIE_VOYAGE_DIMENSIONS`, `CASSIE_VOYAGE_TIMEOUT_SECONDS`, `CASSIE_VOYAGE_MAX_BATCH_SIZE`, `CASSIE_VOYAGE_MAX_RETRIES`, `CASSIE_VOYAGE_BASE_URL`
- `CASSIE_COHERE_API_KEY`, `CASSIE_COHERE_MODEL`, `CASSIE_COHERE_DIMENSIONS`, `CASSIE_COHERE_TIMEOUT_SECONDS`, `CASSIE_COHERE_MAX_BATCH_SIZE`, `CASSIE_COHERE_MAX_RETRIES`, `CASSIE_COHERE_BASE_URL`
- `CASSIE_LOCAL_MODEL`, `CASSIE_LOCAL_DIMENSIONS`

## Design Principles

### Single-node first

Cassie prioritizes exceptional single-node performance over distributed complexity. Every feature, optimization, and storage decision should make a single Cassie node faster, simpler, or more predictable.

### Operational scale over distributed SQL

Cassie scales by adding more nodes, not by distributing individual queries. Its focus is on workload isolation, projection ownership, tenant routing, partition assignment, and horizontal expansion of independent read nodes.

Independent Cassie instances can export offline projection verification manifests for admin consistency checks. These checks compare read-model materialization state across instances; they do not add distributed query execution, replication, or repair.

### Purpose-built for read models

Cassie is optimized for the query patterns that matter in real-world read models, including primary-key lookups, secondary-index lookups, time-range queries, aggregations, reporting workloads, full-text search, vector search, and hybrid search.

### Performance is a feature

Performance is not an afterthought. Query patterns should be benchmarked, measured, and continuously validated against real workloads and explicit latency targets.

### Event-sourcing native

Cassie assumes an event-sourced architecture. Data is projection data, projections can be rebuilt, and recovery is achieved through replay, snapshots, and rebuilds rather than complex database recovery procedures.

### Midge storage layout

Cassie uses the clean-break lexkey v5 Midge key layout for all Cassie-owned storage keys. This is a breaking on-disk layout: existing v4 and older data directories, including flat or slash-delimited keys, `doc:` keys, and `__cassie__` key families, are rejected at startup and must be discarded, restored from a compatible v2 snapshot, or rebuilt from projection sources. Cassie does not migrate them in place.

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

Cassie is not pursuing full PostgreSQL feature parity, distributed SQL execution, distributed transactions, multi-node query planning, stored-procedure business-logic platforms, trigger-based business logic, OLTP optimization, or consensus-heavy clustering. Those concerns belong elsewhere in the architecture.

Limited `CREATE PROCEDURE`/`CALL` support may exist for compatibility and administrative workflows, but procedures are not a product direction for application business logic.

## Vision

Cassie aims to be the simplest, fastest, and most predictable database for building read models. A Cassie deployment should be easy to understand, easy to operate, easy to scale, and capable of serving demanding query workloads with consistent performance.

The event stream owns truth. Cassie owns query performance. Everything else is secondary.
