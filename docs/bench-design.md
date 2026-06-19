Cassie Benchmark Plan

Scope: Cassie query engine only.

Implementation Sequence

The benchmark design is now split into a small, incremental sequence of sprints:

* Sprint 40 - benchmark harness and output contract
* Sprint 41 - Tier 1 hot path microbenchmarks
* Sprint 42 - Tier 2 subsystem benchmarks
* Sprint 43 - Tier 3 end-to-end workload benchmarks
* Sprint 44 - Tier 4 client-facing protocol benchmarks

Tiers

Tier 1 = Hot Path
Tier 2 = Subsystem
Tier 3 = System
Tier 4 = Client-Facing Integration

⸻

Tier 1 — Hot Path

Goal: measure smallest critical operations.

Benchmark:

* row encode/decode
* key encode/decode
* field lookup
* predicate evaluation
* batch filter
* batch projection
* value comparison
* top-k update
* tokenization
* BM25 score
* cosine distance
* dot product
* L2 distance
* query parameter binding
* row-to-Postgres-wire encoding
* row-to-JSON encoding

Metrics:

* ns/op
* allocations/op
* bytes/op
* rows/sec
* vectors/sec

⸻

Tier 2 — Subsystem

Goal: measure complete query-engine components in isolation.

Benchmark:

* SQL lexing
* SQL parsing
* SQL binding
* logical planning
* physical planning
* simple scan executor
* indexed filter executor
* full-text search executor
* vector brute-force executor
* hybrid executor
* projection write path
* index update path
* Postgres wire protocol handler
* HTTP handler

Metrics:

* p50 / p95 / p99
* throughput
* allocations
* memory growth
* rows scanned
* candidates scored
* indexes touched

⸻

Tier 3 — System

Goal: measure Cassie end-to-end inside one running service boundary, without focusing on external client protocol overhead.

Benchmark:

* ingest projection rows
* simple SQL query
* indexed filter query
* range query
* sort + limit
* full-text search
* vector search
* hybrid search
* concurrent queries
* mixed ingest + query load
* projection rebuild
* index rebuild
* cold start
* warm start

Datasets:

* 10k rows
* 1M rows
* 10M rows

Metrics:

* end-to-end latency
* query p50 / p95 / p99
* throughput
* ingest rate
* rebuild time
* memory usage
* CPU utilization
* cache hit rate
* error rate

⸻

Tier 4 — Client-Facing Integration

Goal: measure Cassie exactly as users consume it.

PostgreSQL Wire

Benchmark:

* TCP connection setup
* authentication
* simple query protocol
* prepared statements
* parse/bind/execute/sync
* concurrent connections
* pooled connections
* connection churn
* large result sets
* small result sets
* full-text search over pgwire
* vector search over pgwire
* hybrid search over pgwire

Metrics:

* p50 / p95 / p99
* queries/sec
* rows/sec returned
* bytes/sec encoded
* connections/sec
* prepared statement throughput
* CPU
* memory
* error rate

HTTP

Benchmark:

* POST /sql/query
* POST /search/query
* POST /vector/query
* POST /hybrid/query
* concurrent HTTP requests
* large result sets
* small result sets
* JSON serialization overhead

Metrics:

* p50 / p95 / p99
* req/sec
* rows/sec returned
* bytes/sec encoded
* CPU
* memory
* error rate

Protocol Comparison

Run equivalent workloads through both:

PostgreSQL wire
HTTP

Compare:

* total latency
* serialization cost
* protocol overhead
* throughput
* CPU per query
* memory per query

Tier 4 answers:

Can real clients consume Cassie efficiently over both Postgres wire and HTTP?

⸻

Workload Classes

Point Lookup

SELECT *
FROM applications
WHERE id = $1;

Equality Filter

SELECT *
FROM applications
WHERE status = 'approved'
LIMIT 100;

Range Filter

SELECT *
FROM applications
WHERE created_at >= $1
LIMIT 100;

Sort + Limit

SELECT *
FROM applications
ORDER BY created_at DESC
LIMIT 50;

Full-Text Search

SELECT id, title, search_score(body, $1) AS score
FROM documents
WHERE search(body, $1)
ORDER BY score DESC
LIMIT 20;

Vector Search

SELECT id, title
FROM documents
ORDER BY cosine_distance(embedding, $1)
LIMIT 20;

Hybrid Search

SELECT id, title,
       hybrid_score(
         search_score(body, $1),
         vector_score(embedding, $2)
       ) AS score
FROM documents
WHERE search(body, $1)
ORDER BY score DESC
LIMIT 20;

⸻

Required Output

Every benchmark must emit:

{
  "tier": 1,
  "name": "batch_filter",
  "dataset": "1m_rows",
  "rows": 1000000,
  "duration_ms": 0,
  "p50_ms": 0,
  "p95_ms": 0,
  "p99_ms": 0,
  "throughput": 0,
  "allocations": 0,
  "bytes_allocated": 0,
  "cpu_percent": 0,
  "memory_mb": 0
}

⸻

Acceptance Criteria

Cassie benchmark coverage is complete when:

* Tier 1 isolates critical hot paths.
* Tier 2 measures each query subsystem.
* Tier 3 measures full query-engine behavior.
* Tier 4 measures real client access over Postgres wire and HTTP.
* Equivalent SQL/search/vector/hybrid workloads can be compared across protocols.
* Benchmarks are repeatable in CI.
* Results are emitted in machine-readable format.
