# Performance Contracts

Report date: 2026-06-21

## Purpose

Cassie performance is defined by read-model query patterns, not by vague claims that the database is "fast."
Each supported query shape must declare:

- the product/read-model purpose it serves
- the data assumptions under which the pattern is expected to perform
- explicit latency, throughput, freshness, and memory targets
- the required execution strategy or required projection shape
- the benchmark and regression surface that validates the contract

Optimization work should be triggered by missed contracts, not intuition.

## Core Position

Cassie must compile read-model query patterns into storage-native access paths.
SQL is the interface, not the execution model.

The primary risk is not that Midge is intrinsically slow.
The primary risk is that Cassie hides Midge's strengths behind generic SQL planning and execution choices that discard key locality, bounded scans, prefix access, and streaming behavior.

## Contract Rules

For every supported read-model query pattern:

1. Define the query shape and data assumptions.
2. Set explicit performance targets.
3. Identify the intended execution path.
4. Benchmark the pattern with deterministic fixtures.
5. Optimize only when the pattern misses its contract.
6. Lock the result with regression coverage.

If a read model demands a query shape, Cassie must either:

- provide a performant execution path for that shape, or
- require the projection to materialize the shape directly

This keeps optimization aligned with read-model requirements rather than generic database feature breadth.

A query pattern is not considered supported merely because it returns correct rows.
A query pattern is supported only when it lowers into the expected Midge-efficient access path, or when the required read shape is explicitly materialized so that the access path remains efficient.

## Layer Requirements

| Cassie layer | Requirement |
|--------------|-------------|
| Binder / planner | Recognize Midge-friendly predicates, ranges, ordering, limits, and projection-local read shapes |
| Executor | Prefer streaming scans and bounded reads over row materialization and broad intermediate state |
| Storage adapter | Preserve key locality, prefix/range scan behavior, and filtering advantages exposed by Midge |
| Indexes | Encode expected read-model query shapes directly rather than treating indexes as generic afterthoughts |
| Aggregates | Use precomputed, rollup, aggregate-acceleration, or materialized paths when interactive contracts require them |
| Pagination | Prefer seek/keyset pagination and bounded continuation patterns over offset-driven scans |
| Projection design | Shape stored documents, keys, and derived projections around expected reads |
| Benchmarks | Verify that Cassie uses the intended Midge-oriented access path, not only that a query is low-latency in one fixture |

## Access-Path Assertions

Performance work must include access-path assertions, not just latency assertions.
Phase 04 owns the shared read access-path vocabulary before phase 05 write optimization or phase 06 read implementation consumes it.

Each supported query pattern should define:

- required plan characteristics
- forbidden plan characteristics
- the benchmark that measures the path
- the plan/explain/assertion coverage that proves Cassie chose the right path

Current EXPLAIN and metrics contracts should make the path explicit through labels such as
`access_path`, `fallback_reason`, `pagination_strategy`, `top_k_mode`, `early_stop`, and
`projection_freshness` when applicable. Adaptive read-operator plans also expose
`adaptive_plan_enabled`, `adaptive_decision_point`, `adaptive_candidates`,
`adaptive_selected_alternative`, `adaptive_guard`, `adaptive_guard_passed`, and
`adaptive_reason` so prevalidated alternative selection remains auditable. The guard must name
whether adaptive selection passed because of cost savings or failed because operator-feedback
confidence or savings did not meet the configured threshold. Runtime operator switching exposes
`operator_switch_candidate`, `operator_switch_enabled`, `operator_switch_pair`,
`operator_switch_threshold`, and `operator_switch_reason`; supported switch pairs must define a
replay or transfer state that prevents duplicate or skipped emitted rows. Join-sensitive plans also
expose `join_strategy`, `join_keys`, `join_sort_required`, `join_fallback_reason`,
`vectorized_join_candidate`, `vectorized_join_enabled`, `vectorized_join_batch_size`, and
`vectorized_join_fallback_reason` so merge/vectorized join selection and fallbacks are visible.
For the current scalar and ordered-read scope, that vocabulary includes
`point_lookup`, `index_seek`, `prefix_scan`, `range_scan`, `ordered_bounded_scan`,
`storage_top_k`, `keyset`, and degraded `offset` paths.
The implemented filtered-page baseline includes composite scalar indexes with equality prefixes
and a single range/order field, such as tenant/status filters ordered by a timestamp.
Mixed-direction suffix pages with a selective equality prefix may use a prefix index scan followed
by the normal SQL sort/limit; these plans do not push the SQL limit into storage. Full storage-order
proof remains intentionally narrow: ORDER BY terms over equality-bound leading index fields may use
any direction, and a storage-limited suffix must match the scalar index order with one direction.
Exact equality predicates on deterministic scalar expression indexes lower to
`index_seek`/`prefix_scan`; single-key deterministic scalar expression ranges lower to
`range_scan`; exact single-key deterministic scalar expression ordering with a limit lowers to
`ordered_bounded_scan`. Collation and generic PostgreSQL expression equivalence are not claimed.

This is necessary because a query can look fast at 10k rows while already using the wrong execution model.
The contract should fail before that mistake reaches larger scales.

## Runtime-Boundary Assertions

Runtime-boundary work must include async/sync assertions, not just response correctness assertions.

Each supported async entrypoint should define:

- required async ownership, such as socket IO, HTTP body collection, shutdown, and task coordination
- required synchronous ownership, such as query execution, catalog access, storage, auth verification, and embedding providers
- the blocking boundary that protects Tokio worker tasks
- forbidden direct-blocking behavior inside async transport tasks
- the tests, diagnostics, or static audits that prove the boundary remains explicit

This is necessary because a pgwire or REST path can look correct while blocking the async runtime with planner, executor, storage, Argon2, provider HTTP, or retry sleep work.
Phase 04 runtime-boundary work should fail the contract when Cassie hides synchronous engine work behind async transport code.

For runtime-boundary patterns, pgwire and REST are async interfaces.
They are not a requirement to make the engine itself async.

## Runtime-Boundary Contract (Phase 04)

Required terms:

- async transport task: task that owns socket I/O or HTTP body collection.
- synchronous engine call: planner/executor/catalog/auth/provider logic that executes on dedicated worker threads.
- blocking boundary: an explicit `spawn_blocking` transition between async and synchronous ownership.
- blocking pool: bounded scheduler-backed worker execution used for blocking boundaries.
- boundary timeout: cancellation and shutdown behavior for in-flight blocking work.
- degraded boundary: boundary path that returns `SERVICE_UNAVAILABLE` or error when blocking work cannot run.
- direct-blocking violation: any direct call to sync engine/auth/storage/embedding logic from an async transport loop.

Current boundary ownership:

| Sync entrypoint | Async owner | Sync owner | Blocking owner | Required boundary name | Forbidden direct behavior |
| --- | --- | --- | --- | --- | --- |
| pgwire listener accept | `src/pgwire/server.rs` | transport parse/write only | socket tasks | `run` / `run_with_shutdown` | blocking IO work in accept loop |
| pgwire startup + protocol | `src/pgwire/connection.rs`, `src/pgwire/connection/*` | `run_connection` | query auth/parse/execute modules | `pgwire_auth`, `pgwire_simple_query`, `pgwire_describe`, `pgwire_extended_query`, `pgwire_copy_parse`, `pgwire_copy_from_stdin` | inline `authenticate_role`, `execute_sql`, `describe_parsed_statement`, `execute_parsed_sql_with_mode`, `copy_from_csv_stdin` |
| REST listener accept | `src/rest/router.rs` | `run_with_shutdown`, `route` | HTTP body handling | `run` / `run_with_shutdown` | blocking accept handler loops |
| REST public/admin routes | `src/rest/router.rs` | routing + body collection | route handlers in `run_rest_blocking` | `rest_route`, `rest_embedding_search`, `rest_auth` | inline `collections`, `documents`, `indexes`, `search`, manifest export, manifest comparison, auth/lookup calls |
| Shutdown paths | `src/main.rs`, `src/pgwire/server.rs`, `src/rest/router.rs` | signal/shutdown listeners | runtime notification and task finish | `pgwire/shutdown`, `rest/shutdown` | unbounded task abandonment on signal |

## Runtime-Boundary Validation Ownership

Validation is owned by the same transport slice:

- pgwire boundary behavior: `tests/pgwire_simple_query.rs`, `tests/pgwire_extended_prepared.rs`, `tests/pgwire_extended_execution.rs`, `tests/pgwire_extended_control.rs`, `tests/pgwire_extended_lifecycle.rs`, `tests/pgwire_startup.rs`, `tests/compatibility_matrix.rs`, `tests/metrics_runtime.rs`.
- REST boundary behavior: `tests/rest.rs`, `tests/rest_embeddings.rs`, `tests/rest_metrics.rs`, and `tests/metrics_runtime.rs`.
- static/diff audit: `tests/transport_boundaries.rs` contains a focused scriptable check that verifies transport modules do not call synchronous engine/auth/search/storage APIs directly outside approved blocking helpers.

## Write-Path Assertions

Write optimization must include write-path assertions, not just throughput assertions.

Each supported write pattern should define:

- required row, index, metadata, checkpoint, and rebuild write behavior
- forbidden generic write behavior
- the benchmark that measures the path
- the counters or diagnostics that prove Cassie used the intended path

This is necessary because a replay or rebuild workflow can look fast in a small fixture while still using per-row catalog lookup, duplicate row rewrites, unnecessary index rewrites, or active-target rebuild writes.
Phase 05 write work should fail the contract when Cassie hides Midge locality behind a generic mutation loop.

For write-side patterns, SQL, REST, replay, and rebuild commands are interfaces.
They are not the required execution model.

## Write Pattern Contracts

Write patterns must be explicit, measurable, and tied to read-side expectations.

### Write Pattern: Single projection mutation

### Purpose
Serve interactive CRUD and admin mutations against a projection table with predictable row/index side effects.

### Shape
`INSERT`, `UPDATE`, and `DELETE` against a projection collection plus projection-maintained indexes.

### Data assumptions
- Rows/events: small interactive batches, commonly 1 row
- Batch size: 1
- Indexes maintained: collection PK/unique and projection-local secondary indexes
- Read access shape served: `Primary lookup`, `Secondary lookup`
- Freshness/checkpoint behavior: immediate
- Idempotency requirement: idempotent behavior only where SQL/app layer enforces it

### Performance target
- p50/p95/p99 latency: measured per deployment profile
- Throughput: stable under concurrent small write bursts
- Max memory per query: bounded to working set for the mutated row
- Write amplification budget: no additional full scans, no unnecessary secondary maintenance
- Cold-cache behavior: predictable first-write penalty
- Warm-cache behavior: bounded per-row write cost

### Required write strategy
- direct row write on source collection keys
- update only directly affected index entries
- metadata updates are bounded to the touched row set

### Required read-shape compatibility
- preserve primary and secondary lookup key locality for reads on row identity and tenant-scoped identifiers

### Forbidden write strategy
- per-row catalog reload for each mutation
- rewriting unchanged index values
- full collection scans to recompute derived state

### Validation
- Benchmarks: focused mutation fixture in `tier2_subsystem_ingest`
- Assertions: contract-level row/index/metadata write counters once exposed in the phase 05 write-amplification diagnostics surface

### Interactive or bulk?
Interactive mutation.

### Write Pattern: Replay batch ingestion

### Purpose
Apply event-driven projection updates efficiently while preserving checkpoint integrity.

### Shape
`replay_projection_batch` and replay command inputs.

### Data assumptions
- Rows/events: ordered replay events
- Batch size: medium-to-large bounded batches
- Indexes maintained: collection and index-local secondary structures
- Read access shape served: `Primary lookup`, `Secondary lookup`
- Freshness/checkpoint behavior: checkpoint advances only after successful batch write boundary
- Idempotency requirement: must skip already-applied events

### Performance target
- Throughput: bounded by storage write throughput and index maintenance parallelism
- Write amplification budget: grouped writes within batch boundaries
- Max memory per batch: bounded by configured replay batch queue
- Cold-cache behavior: higher first-batch latency acceptable
- Warm-cache behavior: near-linear with batch size and checkpoints

### Required write strategy
- group duplicate checks and mutation writes before checkpoint updates
- fail preflight conflicts before applying any row or index writes
- apply grouped row/index writes before checkpoint commit

### Required read-shape compatibility
- preserve ordered replay locality for replayed range and ordered-page reads

### Forbidden write strategy
- per-event global flushes
- synchronous dependency on synchronous reads outside the replay path

### Validation
- Benchmarks: `projection_lag_catchup` and `projection_write_path` in `tier2_subsystem_ingest`
- Assertions: checkpoint progression, preflight conflict isolation, and bounded duplicate check overhead

### Interactive or bulk?
Bulk replay path.

### Write Pattern: Duplicate replay skip

### Purpose
Avoid write amplification when replay events are redelivered.

### Shape
Replay input with repeated event IDs, sequence pairs, or checkpoint values.

### Data assumptions
- Rows/events: duplicate delivery windows under active retries
- Batch size: mixed with normal replay
- Indexes maintained: source projection indexes and checkpoint metadata
- Read access shape served: `Primary lookup`
- Freshness/checkpoint behavior: no mutation when duplicate detected
- Idempotency requirement: explicit

### Performance target
- p50/p95/p99 latency: skip decision must be O(1) to O(log n) on checkpoint/index lookup
- Throughput: no degradation from duplicate flood
- Max memory per query: bounded by checkpoint/index lookup set
- Write amplification budget: zero row/index writes for skipped events

### Required write strategy
- detect duplicates before row/index mutation
- reject duplicate new event ids inside the same replay batch as a conflict
- emit duplicate skip counters before moving cursor

### Required read-shape compatibility
- preserve duplicate-index and checkpoint read locality required by both single and replayed lookups

### Forbidden write strategy
- re-writing data for duplicate events
- appending duplicate audit rows outside replay telemetry contract

### Validation
- Benchmarks: `projection_duplicate_replay` in `tier2_subsystem_ingest`
- Assertions: duplicate check counters and no duplicate side-effect writes

### Interactive or bulk?
Replay guard, applied in both interactive replay and bulk replay.

### Write Pattern: Indexed mutation

### Purpose
Support high-frequency reads on indexed projections while minimizing index churn.

### Shape
Any indexed column mutation path in insert/update/delete flows.

### Data assumptions
- Rows/events: interactive or batch updates to indexed fields
- Batch size: 1 to many
- Indexes maintained: scalar, full-text, vector, or hybrid candidate indexes as defined by schema
- Read access shape served: `Secondary lookup`, `Filtered page`, `Ordered page`, `Hybrid search`, `Vector search`
- Freshness/checkpoint behavior: immediate metadata and index state
- Idempotency requirement: depends on upstream mutation semantics

### Performance target
- p50/p95/p99 latency: bounded by index delta write path size
- Throughput: stable under mixed update and lookup load
- Write amplification budget: avoid full index rebuild for single key updates

### Required write strategy
- update or delete only affected index entries for touched keys
- keep key encoding and grouping aligned with the archived read access-path contract surface in this document

### Required read-shape compatibility
- maintain index localities used by required filtered and ordered reads

### Forbidden write strategy
- full index backfill for single-row mutations
- reorder/overwrite entire index blocks outside changed key range

### Validation
- Benchmarks: index update-focused ingest benchmark in `tier2_subsystem_ingest`
- Assertions: index write counters stay bounded for single-key updates

### Interactive or bulk?
Interactive and small batch mutation path.

### Write Pattern: Projection refresh / build

### Purpose
Rebuild materialized projection target from source events or source-of-truth reads.

### Shape
Projection materialized refresh and projection rebuild commands.

### Data assumptions
- Rows/events: full projection target rewrite windows
- Batch size: large ordered source ranges
- Indexes maintained: projection-local read-path indexes
- Read access shape served: `Projection replay`, `Projection rebuild`, `Join-like reads`
- Freshness/checkpoint behavior: old target remains readable until swap
- Idempotency requirement: explicit on command retries

### Performance target
- Throughput: high write throughput with bounded metadata overhead
- Max memory per batch: bounded by target table scan buffers
- Write amplification budget: no active-version active-target rewrite
- Cold-cache behavior: full rebuild warm-up expected

### Required write strategy
- write into inactive rebuild target
- validate, checkpoint, and swap only after full consistency checks

### Required read-shape compatibility
- preserve projection-local ordering/grouping expected by rebuilt read paths

### Forbidden write strategy
- in-place overwrite of active projection while rebuilding
- broad checkpoint mutation before rebuild completeness

### Validation
- Benchmarks: `projection_refresh` and `projection_rebuild_query` in `tier3_system_rebuild`
- Assertions: swap phase separated from rebuild throughput

### Interactive or bulk?
Bulk rebuild path.

### Write Pattern: Projection verification-adjacent rebuild

### Purpose
Support verification and comparison workflows that refresh only changed structures.

### Shape
Projection verification command paths and mismatch-ledger repair steps.

### Data assumptions
- Rows/events: verification and repair batches
- Batch size: bounded by verification window
- Indexes maintained: projection metadata and content hashes
- Read access shape served: `Primary lookup`, `Projection replay`
- Freshness/checkpoint behavior: verified version remains authoritative
- Idempotency requirement: repair actions must be safe if repeated

### Performance target
- Throughput: stable under long-running verification scans
- Write amplification budget: metadata-only updates where no mismatch is detected
- Max memory per verification batch: bounded

### Required write strategy
- preserve hash and verification key layouts
- write minimal metadata updates for verified consistency states

### Required read-shape compatibility
- keep projection identity and hash-index layouts stable for read verification

### Forbidden write strategy
- rewriting source rows during verification
- eager backfill outside mismatch scope

### Validation
- Benchmarks: projection verify-oriented profiles in `tier3_system_rebuild`
- Assertions: verification metadata write counters and mismatch repair scope

### Interactive or bulk?
Offline verification with bounded metadata writes.

### Write Pattern: Version swap-adjacent writes

### Purpose
Move projection versions from inactive to active state without rewriting user data.

### Shape
`projection_version` activation and metadata updates.

### Data assumptions
- Rows/events: one swap request per projection version
- Batch size: O(1) command set
- Indexes maintained: projection version lookup and activation map
- Read access shape served: `Projection rebuild`
- Freshness/checkpoint behavior: atomic activation semantics
- Idempotency requirement: repeated swap attempts are no-ops or safe failures

### Performance target
- p50/p95/p99 latency: low and bounded by metadata writes
- Write amplification budget: metadata-only updates
- Max memory per swap: O(1)

### Required write strategy
- bounded metadata writes for version, epoch, and activation marker
- keep active/read-only projection targets stable during swap

### Required read-shape compatibility
- preserve active-version-locality assumptions for replay and query routing

### Forbidden write strategy
- active target rewrite during swap
- unrelated projection data mutation during activation

### Validation
- Benchmarks: `projection_swap` in `tier3_system_rebuild`
- Assertions: activation writes only, no unrelated row/index churn

### Interactive or bulk?
Controlled control-plane path.

### Write Pattern: Index rebuild DDL

### Purpose
Rebuild or backfill indexes from stable source data without impacting active readers.

### Shape
`CREATE INDEX`, `DROP INDEX`, and index backfill/rebuild command flows.

### Data assumptions
- Rows/events: source projection data for rebuild
- Batch size: source-order stream
- Indexes maintained: scalar/full-text/vector/hybrid index families
- Read access shape served: relevant read contracts in `Primary lookup`, `Secondary lookup`, `Filtered page`, `Full-text search`, `Vector search`, `Hybrid search`
- Freshness/checkpoint behavior: index build has explicit build phase and ready state
- Idempotency requirement: repeatable backfill and drop/recreate behavior

### Performance target
- Throughput: proportional to source scan and index append rate
- Max memory per chunk: bounded by streaming rebuild windows
- Write amplification budget: avoid repeated full rewrites when key range unchanged

### Required write strategy
- stream from source order into new index target
- build and publish index atomically when consistent

### Required read-shape compatibility
- preserve ordering and grouping required by index-backed reads

### Forbidden write strategy
- index mutation in place during active read path migration
- non-deterministic key derivation across rebuild runs

### Validation
- Benchmarks: `index_rebuild_ddl` in `tier3_system_rebuild`
- Assertions: backfill progress, target size, and final consistency checks

### Interactive or bulk?
Bulk/admin path with controlled publication boundary.

## Write Amplification Vocabulary

Phase 05 counters to expose for contracts and diagnostics:

- `row_puts` (exact): row insert/update operations on source projection/collection storage.
- `row_deletes` (exact): row delete operations.
- `index_puts` (exact): index entry inserts/updates.
- `index_deletes` (exact): index entry removals.
- `metadata_puts` (exact): metadata writes and checkpoint updates.
- `metadata_deletes` (exact): metadata deletions where command explicitly removes metadata state.
- `duplicate_checks` (exact): replay duplicate or idempotency lookups.
- `duplicates_skipped` (exact): replay events skipped due to duplicate detection.
- `batch_flushes` (exact): explicit storage or batching flush boundaries.
- `rebuild_target_puts` (exact): inactive target writes during projection/index rebuild.
- `activation_metadata_writes` (exact): swap and activation marker updates.

Derived ratios:

- `storage_writes_per_replay_event = (row_puts + row_deletes + index_puts + index_deletes) / replay_events_applied`.
- `index_writes_per_row_mutation = (index_puts + index_deletes) / max(row_puts + row_deletes, 1)`.
- `metadata_writes_per_replay_batch = metadata_puts / replay_batches`.
- `activation_writes_per_swap = (metadata_puts + activation_metadata_writes) / max(swaps, 1)`.

## Requirement Template

Use this template when adding or revising a supported query pattern.

```md
## Query Pattern: <name>
### Purpose
What product/read-model need this serves.

### Shape
Example SQL.

### Data assumptions
- Rows/documents:
- Cardinality:
- Selectivity:
- Indexes expected:
- Projection freshness:

### Performance target
- p50 latency:
- p95 latency:
- p99 latency:
- Throughput:
- Max result size:
- Max memory per query:
- Cold-cache behavior:
- Warm-cache behavior:

### Required execution strategy
Index scan, range scan, full-text index, vector index, hybrid search, column batch, aggregate path, projection materialization, etc.

### Non-goals
Explicitly unsupported or degraded cases.

### Validation
Benchmark name, fixture size, expected assertions.

### Required access-path assertions
- Required plan shape:
- Forbidden plan shape:
- Explain/assertion coverage:
```

## Contract Categories

These categories define the minimum performance-contract surface for Cassie V1 read models.

| Category | Representative shapes |
|----------|------------------------|
| Primary lookup | `WHERE id = ?` |
| Secondary lookup | `WHERE tenant_id = ? AND external_id = ?` |
| Range scan | `WHERE created_at BETWEEN ? AND ?` |
| Ordered page | `ORDER BY created_at DESC LIMIT 50` |
| Filtered page | tenant/status/date filters |
| Count / exists | `COUNT(*)`, `EXISTS` |
| Aggregates | grouped totals, sums, buckets |
| Full-text search | keyword search over docs |
| Vector search | nearest-neighbor lookup |
| Hybrid search | text + vector + structured filters |
| Time bucket | daily/hourly buckets |
| Column batch | analytical read-model scans |
| Projection replay | idempotent event replay, duplicate handling, lag catch-up |
| Projection rebuild | materialized refresh, rebuild verification, version swap |
| Join-like reads | preferably pre-projected rather than runtime-heavy joins |

## Projection Lifecycle Benchmarks

Phase 02 adds compile-validated benchmark coverage for projection lifecycle costs at the existing 10k fixture scale.

| Workflow | Benchmark |
| --- | --- |
| Replay ingestion write path | `cargo bench --locked --bench tier2_subsystem_ingest` |
| Duplicate replay handling | `projection_duplicate_replay` in `tier2_subsystem_ingest` |
| Lag catch-up replay | `projection_lag_catchup` in `tier2_subsystem_ingest` |
| Materialized projection refresh | `projection_refresh` in `tier3_system_rebuild` |
| Rebuild verification | `projection_verify` in `tier3_system_rebuild` |
| Version swap latency | `projection_swap` in `tier3_system_rebuild` |

Initial targets are comparative rather than SLA-grade: verification and swap costs must be visible separately from rebuild/query costs, fixtures must be deterministic, and benchmarks must not require services outside Cassie and Midge.

## Pattern Contracts

The first 10k/100k performance slice is a manual developer feedback loop, not a CI gate or SLA table.
Each scenario below has a stable Criterion owner, fixture scale, scenario id, and evidence labels so a developer can run the relevant benchmark while working on a read-model path and compare local p50, p95, p99, and throughput movement over time.

Manual benchmark runs are opt-in so default `cargo test` does not execute expensive fixtures:

```sh
cargo bench --locked --bench tier3_system_query
cargo bench --locked --bench tier2_subsystem_ingest
cargo bench --locked --bench tier3_system_rebuild
cargo bench --locked --bench tier2_subsystem_search
cargo bench --locked --bench tier2_subsystem_vector
cargo bench --locked --bench tier2_subsystem_hybrid
cargo bench --locked --bench tier2_subsystem_executor
cargo bench --locked --bench tier4_integration_pgwire
cargo bench --locked --bench tier4_integration_http
cargo test --locked --test performance_benchmarks -- --ignored --nocapture
```

For CI jobs or quick local checks that only need to confirm benchmark ownership still compiles without executing fixtures:

```sh
cargo bench --locked --bench tier3_system_query --no-run
cargo bench --locked --bench tier2_subsystem_ingest --no-run
cargo bench --locked --bench tier3_system_rebuild --no-run
cargo bench --locked --bench tier2_subsystem_search --no-run
cargo bench --locked --bench tier2_subsystem_vector --no-run
cargo bench --locked --bench tier2_subsystem_hybrid --no-run
cargo bench --locked --bench tier2_subsystem_executor --no-run
cargo bench --locked --bench tier4_integration_pgwire --no-run
cargo bench --locked --bench tier4_integration_http --no-run
```

The benchmark fixture uses `CASSIE_MIDGE_ALLOW_FALLBACK=1` and forces Midge's in-memory fallback by default so local results are repeatable and do not measure host disk sync behavior.
Set `BENCH_MIDGE_DISK=1` before the `cargo bench` commands when collecting disk-backed exploratory numbers.

The ignored `performance_benchmarks` test reads Criterion `sample.json` files from `target/criterion` and prints one line per scenario:

```text
perf.core_read.simple.10k profile=local-dev-fallback-10k benchmark=tier3_system_query workload=simple_sql_query scale=10k storage=in_memory_midge_fallback p50=...us p95=...us p99=...us throughput=...ops/s fallback_evidence=fallback_reason cache_evidence=plan_cache.entries storage_evidence=storage.data.reads feature_evidence=query.latency_ms_total non_goals=not_sla|not_ci_gate|not_production_ready_promotion|not_disk_sync_unless_bench_midge_disk
```

### Deployment Profile Evidence

Benchmark output is evidence collection, not a production-ready promotion or SLA claim.
A deployment profile records enough context for a future reviewer to compare benchmark output against a declared environment and workload.

| Profile | Host shape | Storage mode | Data shape | Workload mix | Fixture scale | Benchmark command | Metrics captured | Known non-goals |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `local-dev-fallback-10k` | Local developer workstation | `in_memory_midge_fallback` | Deterministic generated read-model fixture | Single benchmark owner workload | 10k | `cargo bench --locked --bench <owner-benchmark>` | p50, p95, p99, throughput, fallback counters, cache occupancy, storage-family operations, feature-family metrics | Not SLA, not CI gate, not production-ready promotion, not disk sync unless `BENCH_MIDGE_DISK=1` |
| `local-dev-fallback-100k` | Local developer workstation | `in_memory_midge_fallback` | Deterministic generated read-model fixture | Single benchmark owner workload | 100k | `cargo bench --locked --bench <owner-benchmark>` | p50, p95, p99, throughput, fallback counters, cache occupancy, storage-family operations, feature-family metrics | Not SLA, not CI gate, not production-ready promotion, not disk sync unless `BENCH_MIDGE_DISK=1` |
| `future-1m-placeholder` | Declared deployment profile | `profile-defined` | Future generated read-model fixture | Single benchmark owner workload | 1M | `cargo bench --locked --bench <owner-benchmark> --no-run` | p50, p95, p99, throughput, fallback counters, cache occupancy, storage-family operations, feature-family metrics | Not SLA, not CI gate, not production-ready promotion, not a default fixture, not required by current benchmarks |

The 1M profile is a compile-validated placeholder only.
It reserves representative future scenario ids without requiring 1M fixture generation or Criterion output in the default manual workflow:
`perf.core_read.simple.1m`, `perf.replay.lag_catchup.1m`, `perf.rebuild.refresh.1m`, `perf.verification.full.1m`, `perf.search.fulltext.1m`, `perf.vector.executor.1m`, `perf.hybrid.executor.1m`, `perf.graph.expand.1m`, `perf.time_series.window_scan.1m`, `perf.pgwire.simple_query.1m`, and `perf.http.document_create_get.1m`.

### TDD Optimization Round 01

This round targets read-model hot paths rather than general OLAP parity. Each change must start with
a failing or newly guarding test that asserts result correctness plus EXPLAIN/metrics evidence, then
add or update Criterion ownership before optimizing implementation code.

Round 01 owns these hot paths:

- tenant/status/time ordered page: `perf.read_path.mixed_order.10k` and `perf.read_path.mixed_order.100k`
- mixed-direction suffix ordered page: `perf.read_path.mixed_direction_suffix.10k` and `perf.read_path.mixed_direction_suffix.100k`
- expression-index equality lookup: `perf.read_path.expression_index.10k` and `perf.read_path.expression_index.100k`
- expression-index range scan: `perf.read_path.expression_index_range.10k` and `perf.read_path.expression_index_range.100k`
- expression-index ordered bounded scan: `perf.read_path.expression_index_order.10k` and `perf.read_path.expression_index_order.100k`
- column-batch covered projection/filter: `perf.read_path.column_batch.10k` and `perf.read_path.column_batch.100k`
- vectorized inner/left equi-join when explicitly enabled: `perf.read_path.vectorized_join.10k` and `perf.read_path.vectorized_join.100k`
- bounded unordered vectorized left join: `perf.read_path.vectorized_left_join_limited.10k` and `perf.read_path.vectorized_left_join_limited.100k`
- bounded streaming vectorized inner join without a left-key index: `perf.read_path.vectorized_streaming_inner_join.10k` and `perf.read_path.vectorized_streaming_inner_join.100k`
- dense bounded streaming vectorized inner join under tight temp budget: `perf.read_path.vectorized_dense_streaming_inner_join.10k` and `perf.read_path.vectorized_dense_streaming_inner_join.100k`
- bounded indexed vectorized inner join: `perf.read_path.vectorized_indexed_inner_join.10k` and `perf.read_path.vectorized_indexed_inner_join.100k`
- bounded right-indexed vectorized inner join: `perf.read_path.vectorized_right_indexed_inner_join.10k` and `perf.read_path.vectorized_right_indexed_inner_join.100k`
- missing-stats late-match bounded vectorized inner join: `perf.read_path.vectorized_late_match_inner_join.10k` and `perf.read_path.vectorized_late_match_inner_join.100k`
- row-count-sampled larger-output bounded vectorized inner join: `perf.read_path.vectorized_fanout_inner_join.10k` and `perf.read_path.vectorized_fanout_inner_join.100k`
- pgwire prepared read path: `perf.pgwire.prepared_query.10k` and `perf.pgwire.prepared_query.100k`

The default decision rule is to prefer Midge-native access paths, covering reads, bounded scans,
column-batch pruning, and selective vectorized operators before broader executor rewrites.

Round 01 measured notes from 2026-06-27 local-dev fallback runs:

- `vectorized_join_equi/100k` now reaches stable Criterion measurement instead of stalling in fixture setup; latest mean was 36.309 us with a 35.909-36.885 us confidence interval.
- `vectorized_left_join_limited` validates bounded left-source scans: 35.604 us at 10k and 36.037 us at 100k.
- `vectorized_streaming_inner_join` validates bounded streaming left-source scans for sparse unindexed inner joins: 36.597 us at 10k and 36.672 us at 100k.
- `vectorized_dense_streaming_inner_join` validates dense bounded nested-stream scans under tight temp budget: 34.853 us at 10k and 34.631 us at 100k.
- `vectorized_indexed_inner_join` validates indexed left-source probes for bounded inner joins: local 2026-07-02 advisory run measured 63.365-65.769 us at 10k and 63.236-67.994 us at 100k.
- `vectorized_right_indexed_inner_join` validates indexed right-source probes for bounded inner joins: local 2026-07-02 advisory run measured 54.349-56.399 us at 10k and 54.332-55.440 us at 100k.
- `vectorized_late_match_inner_join` validates bounded row-count probes for missing-stats asymmetric bounded joins: local 2026-07-02 advisory run measured 51.769-53.156 us at 10k and 51.793-53.125 us at 100k.
- `vectorized_fanout_inner_join` validates row-count-sampled build-side selection when join-key field stats are unavailable for larger-output asymmetric bounded joins: local 2026-07-02 advisory run measured 4.7098-5.6266 ms at 10k and 4.4832-4.9095 ms at 100k.
- `expression_index_range_query` validates scalar expression range scans: 22.284 us at 10k and 21.667 us at 100k after explicit benchmark warmup.
- `expression_index_order_query` validates scalar expression ordered bounded scans: 9.663 us at 10k and 9.812 us at 100k after explicit benchmark warmup.
- `mixed_direction_scalar_query` validates prefix index scans followed by final sort for mixed-direction suffix ordering: local 2026-07-02 advisory run measured 58.263-59.392 us at 10k and 82.606-85.997 us at 100k.
- `pgwire_prepared_query` remained scale-flat: 54.234 us at 10k and 55.031 us at 100k.
- `column_batch_covered_projection` now measures with tight intervals after explicit benchmark warmup: 16.202 us at 10k and 15.354 us at 100k.
- Remaining executor bottleneck: non-indexed bounded joins that cannot prove a bounded left build through hydrated row counts, capped runtime row-count probes, or supportive join-key samples still need deeper adaptive side selection before both inputs can avoid broad materialization safely.

### Manual Benchmark Scenarios

| Scenario | Family | Owner benchmark | Workload | Scale |
| --- | --- | --- | --- | --- |
| `perf.core_read.simple.10k` | Core read | `tier3_system_query` | `simple_sql_query` | 10k |
| `perf.core_read.simple.100k` | Core read | `tier3_system_query` | `simple_sql_query` | 100k |
| `perf.read_path.mixed_order.10k` | Core read | `tier3_system_query` | `mixed_order_scalar_query` | 10k |
| `perf.read_path.mixed_order.100k` | Core read | `tier3_system_query` | `mixed_order_scalar_query` | 100k |
| `perf.read_path.mixed_direction_suffix.10k` | Core read | `tier3_system_query` | `mixed_direction_scalar_query` | 10k |
| `perf.read_path.mixed_direction_suffix.100k` | Core read | `tier3_system_query` | `mixed_direction_scalar_query` | 100k |
| `perf.read_path.expression_index.10k` | Core read | `tier3_system_query` | `expression_index_query` | 10k |
| `perf.read_path.expression_index.100k` | Core read | `tier3_system_query` | `expression_index_query` | 100k |
| `perf.read_path.expression_index_range.10k` | Core read | `tier3_system_query` | `expression_index_range_query` | 10k |
| `perf.read_path.expression_index_range.100k` | Core read | `tier3_system_query` | `expression_index_range_query` | 100k |
| `perf.read_path.expression_index_order.10k` | Core read | `tier3_system_query` | `expression_index_order_query` | 10k |
| `perf.read_path.expression_index_order.100k` | Core read | `tier3_system_query` | `expression_index_order_query` | 100k |
| `perf.read_path.column_batch.10k` | Core read | `tier2_subsystem_executor` | `column_batch_covered_projection` | 10k |
| `perf.read_path.column_batch.100k` | Core read | `tier2_subsystem_executor` | `column_batch_covered_projection` | 100k |
| `perf.read_path.vectorized_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_join_equi` | 10k |
| `perf.read_path.vectorized_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_join_equi` | 100k |
| `perf.read_path.vectorized_left_join_limited.10k` | Core read | `tier2_subsystem_executor` | `vectorized_left_join_limited` | 10k |
| `perf.read_path.vectorized_left_join_limited.100k` | Core read | `tier2_subsystem_executor` | `vectorized_left_join_limited` | 100k |
| `perf.read_path.vectorized_streaming_inner_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_streaming_inner_join` | 10k |
| `perf.read_path.vectorized_streaming_inner_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_streaming_inner_join` | 100k |
| `perf.read_path.vectorized_dense_streaming_inner_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_dense_streaming_inner_join` | 10k |
| `perf.read_path.vectorized_dense_streaming_inner_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_dense_streaming_inner_join` | 100k |
| `perf.read_path.vectorized_indexed_inner_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_indexed_inner_join` | 10k |
| `perf.read_path.vectorized_indexed_inner_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_indexed_inner_join` | 100k |
| `perf.read_path.vectorized_right_indexed_inner_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_right_indexed_inner_join` | 10k |
| `perf.read_path.vectorized_right_indexed_inner_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_right_indexed_inner_join` | 100k |
| `perf.read_path.vectorized_late_match_inner_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_late_match_inner_join` | 10k |
| `perf.read_path.vectorized_late_match_inner_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_late_match_inner_join` | 100k |
| `perf.read_path.vectorized_fanout_inner_join.10k` | Core read | `tier2_subsystem_executor` | `vectorized_fanout_inner_join` | 10k |
| `perf.read_path.vectorized_fanout_inner_join.100k` | Core read | `tier2_subsystem_executor` | `vectorized_fanout_inner_join` | 100k |
| `perf.replay.lag_catchup.10k` | Replay | `tier2_subsystem_ingest` | `projection_lag_catchup` | 10k |
| `perf.replay.lag_catchup.100k` | Replay | `tier2_subsystem_ingest` | `projection_lag_catchup` | 100k |
| `perf.rebuild.refresh.10k` | Rebuild | `tier3_system_rebuild` | `projection_refresh` | 10k |
| `perf.rebuild.refresh.100k` | Rebuild | `tier3_system_rebuild` | `projection_refresh` | 100k |
| `perf.verification.full.10k` | Verification | `tier3_system_rebuild` | `projection_verify` | 10k |
| `perf.verification.full.100k` | Verification | `tier3_system_rebuild` | `projection_verify` | 100k |
| `perf.search.fulltext.10k` | Search | `tier2_subsystem_search` | `full_text_executor` | 10k |
| `perf.search.fulltext.100k` | Search | `tier2_subsystem_search` | `full_text_executor` | 100k |
| `perf.vector.executor.10k` | Vector | `tier2_subsystem_vector` | `vector_executor` | 10k |
| `perf.vector.executor.100k` | Vector | `tier2_subsystem_vector` | `vector_executor` | 100k |
| `perf.hybrid.executor.10k` | Hybrid | `tier2_subsystem_hybrid` | `hybrid_executor` | 10k |
| `perf.hybrid.executor.100k` | Hybrid | `tier2_subsystem_hybrid` | `hybrid_executor` | 100k |
| `perf.graph.expand.10k` | Graph | `tier3_system_query` | `graph_expand_query` | 10k |
| `perf.graph.expand.100k` | Graph | `tier3_system_query` | `graph_expand_query` | 100k |
| `perf.time_series.window_scan.10k` | Time series | `tier3_system_query` | `time_series_window_scan` | 10k |
| `perf.time_series.window_scan.100k` | Time series | `tier3_system_query` | `time_series_window_scan` | 100k |
| `perf.time_series.retention.10k` | Time series | `tier3_system_rebuild` | `time_series_retention_enforcement` | 10k |
| `perf.time_series.retention.100k` | Time series | `tier3_system_rebuild` | `time_series_retention_enforcement` | 100k |
| `perf.time_series.rollup_refresh.10k` | Time series | `tier3_system_rebuild` | `time_series_rollup_refresh` | 10k |
| `perf.time_series.rollup_refresh.100k` | Time series | `tier3_system_rebuild` | `time_series_rollup_refresh` | 100k |
| `perf.pgwire.simple_query.10k` | pgwire | `tier4_integration_pgwire` | `pgwire_simple_query` | 10k |
| `perf.pgwire.simple_query.100k` | pgwire | `tier4_integration_pgwire` | `pgwire_simple_query` | 100k |
| `perf.pgwire.prepared_query.10k` | pgwire | `tier4_integration_pgwire` | `pgwire_prepared_query` | 10k |
| `perf.pgwire.prepared_query.100k` | pgwire | `tier4_integration_pgwire` | `pgwire_prepared_query` | 100k |
| `perf.http.document_create_get.10k` | HTTP | `tier4_integration_http` | `http_document_create_get` | 10k |
| `perf.http.document_create_get.100k` | HTTP | `tier4_integration_http` | `http_document_create_get` | 100k |
| `perf.http.vector_search.10k` | HTTP | `tier4_integration_http` | `http_vector_search` | 10k |

`time_series_window_scan` owns developer feedback for the bucket-native range path and its row-backed fallback.

### Evidence Labels

| Contract family | Memory/fallback evidence | EXPLAIN or metrics evidence |
| --- | --- | --- |
| Core read | `storage.data.reads`, `fallback_reason`, `column_batches.row_fetches_avoided`, `read_paths.collection_scan_rows`, `joins.vectorized_build_rows_total`, `joins.last_vectorized_fallback_reason` | `access_path=point_lookup`, `access_path=range_scan`, `access_path=index_seek`, `column_batch_index`, `vectorized_join_enabled=true`, `query.latency_ms_total`, `read_paths.range_scans`, `read_paths.index_seek_scans`, `column_batches.scans`, `joins.vectorized_joins` |
| Replay | `projections.write_batch_flushes`, `projections.replay_duplicates_skipped` | `replay_checkpoint`, `projections.replay_events_applied` |
| Rebuild | `projections.write_rebuild_target_puts`, `rebuild_fallback` | `materialized_projection_refresh`, `projections.refreshes` |
| Verification | `projection_hash_rows`, `verification_mismatch_count` | `VERIFY PROJECTION`, `projections.verifications` |
| Search | `search.candidate_count_total`, `search_fallback` | `access_path=fulltext`, `search.latency_ms_total` |
| Vector | `vector.candidate_count_total`, `vector.normalized_fallback_count_total` | `access_path=vector`, `vector.latency_ms_total` |
| Hybrid | `hybrid.candidate_count_total`, `hybrid.prefilter_fallback_count_total` | `mixed_execution`, `hybrid.latency_ms_total` |
| Time series | `time_series.bucket_native_hits`, `time_series.fallback_reason`, `retention.skipped_rows`, `retention.errors`, `rollups.refreshes`, `rollups.stale_fallbacks` | `time_series_storage=bucket-native-v1`, `time_series.buckets_scanned`, `time_series.scans`, `ENFORCE RETENTION`, `retention.deleted_rows`, `REFRESH ROLLUP`, `rollups.rewrite_hits` |
| pgwire | `pgwire.blocking_elapsed_ms_total`, `pgwire.protocol_errors_total` | `pgwire_simple_query`, `pgwire_prepared_query`, `pgwire.simple_queries_total`, `pgwire.extended_queries_total` |
| HTTP | `storage.data.writes`, `vector.normalized_fallback_count_total`, `rest.blocking_error_total` | `documents::create/get`, `http_vector_search`, `rest.requests_total` |

## Example Discipline

### Query Pattern: Tenant ordered page

### Shape
```sql
SELECT *
FROM invoices
WHERE tenant_id = $1
ORDER BY created_at DESC
LIMIT 50;
```

### Required Cassie plan
- use a `tenant_id + created_at` access path
- perform a bounded prefix/range scan
- stream matching rows from Midge
- stop after 50 matches

### Forbidden Cassie plan
- full collection scan
- sort after scan
- materialize all tenant rows
- offset-driven pagination for the interactive path

This is the expected standard for supported read-model query patterns.
Correctness alone is not sufficient.

### Query Pattern: Primary lookup

### Purpose
Serve point reads for projection-backed APIs, admin tools, and application detail views.

### Shape
```sql
SELECT * FROM orders_projection WHERE id = $1;
```

### Data assumptions
- Rows/documents: 10k baseline, then 100k and 1M
- Cardinality: unique identifier
- Selectivity: one row
- Indexes expected: primary key or unique scalar index
- Projection freshness: fresh or explicitly stale-but-readable

### Performance target
- p50 latency: measured and budgeted per scale tier
- p95 latency: measured and budgeted per scale tier
- p99 latency: measured and budgeted per scale tier
- Throughput: stable under concurrent point-read load
- Max result size: one row/document
- Max memory per query: bounded and near-constant
- Cold-cache behavior: documented separately from warm path
- Warm-cache behavior: primary acceptance path

### Required execution strategy
Primary key or unique index lookup. No full scan.

### Non-goals
Large document fetches with heavy computed expressions are not part of the point-read contract.

### Validation
- Benchmarks: add dedicated point-lookup coverage or extend `benches/tier2_subsystem_executor.rs`
- Tests: plan-shape coverage in `tests/planner_indexes.rs`

### Required access-path assertions
- Required plan shape: primary-key or unique-index lookup
- Forbidden plan shape: collection scan before predicate evaluation
- Explain/assertion coverage: planner or explain tests must prove index selection

### Query Pattern: Secondary lookup

### Purpose
Serve tenant-scoped identity lookups and idempotency/read-model correlation reads.

### Shape
```sql
SELECT id, status
FROM orders_projection
WHERE tenant_id = $1 AND external_id = $2;
```

### Data assumptions
- Rows/documents: 10k baseline, then 100k and 1M
- Cardinality: many tenants, unique or near-unique secondary key within tenant
- Selectivity: one or few rows
- Indexes expected: composite scalar index
- Projection freshness: fresh

### Performance target
- p50/p95/p99 latency: explicit per-tier target
- Throughput: stable under concurrent tenant-partitioned lookups
- Max result size: small bounded row set
- Max memory per query: bounded
- Cold/warm behavior: tracked separately

### Required execution strategy
Composite index seek. No tenant-wide scan.

### Non-goals
Cross-tenant lookups without an index are degraded paths and should be documented as such.

### Validation
- Benchmarks: extend executor or protocol handler benches with composite-key predicates
- Tests: index selection coverage in `tests/planner_indexes.rs`

### Required access-path assertions
- Required plan shape: composite index seek on tenant and external identity
- Forbidden plan shape: tenant-partition scan or global scan
- Explain/assertion coverage: plan-shape assertions for composite-key selection

### Query Pattern: Range scan

### Purpose
Serve event-derived timelines, audit trails, and operational history views.

### Shape
```sql
SELECT id, created_at, status
FROM orders_projection
WHERE created_at BETWEEN $1 AND $2
ORDER BY created_at ASC;
```

### Data assumptions
- Rows/documents: 10k baseline, then larger tiers
- Cardinality: high-cardinality timestamp or monotonic key
- Selectivity: bounded time window
- Indexes expected: range-friendly scalar index or projection-local ordering
- Projection freshness: fresh or stale within documented window

### Performance target
- Explicit latency targets per window size and scale point
- Throughput: stable for paging and bounded scans
- Max result size: bounded by caller contract
- Max memory per query: bounded, should not scale with table size
- Cold/warm behavior: documented

### Required execution strategy
Range scan over ordered index or materialized projection shape. No full table scan for routine windows.

### Non-goals
Unbounded historical scans belong to analytical or export paths, not interactive read contracts.

### Validation
- Benchmarks: extend `benches/tier2_subsystem_executor.rs` or `benches/tier3_system_query.rs`
- Tests: planner/index coverage in `tests/planner_indexes.rs`

### Required access-path assertions
- Required plan shape: bounded ordered range scan
- Forbidden plan shape: full scan plus post-filter plus full sort
- Explain/assertion coverage: planner or explain tests for range-aware access

### Query Pattern: Ordered page

### Purpose
Serve dashboard pages and operator list views.

### Shape
```sql
SELECT id, created_at, status
FROM orders_projection
ORDER BY created_at DESC
LIMIT 50;
```

### Data assumptions
- Rows/documents: 10k baseline and higher
- Cardinality: many rows
- Selectivity: top-N page
- Indexes expected: ordering-supporting index or materialized order
- Projection freshness: fresh

### Performance target
- Explicit top-N latency targets
- Throughput: stable for repeated interactive paging
- Max result size: page-sized
- Max memory per query: bounded by top-N strategy
- Cold/warm behavior: documented

### Required execution strategy
Index-backed top-N or projection-preordered read. Avoid full sort for common pages.

### Non-goals
Deep offset pagination without a matching access path is a degraded case.

### Validation
- Benchmarks: `benches/tier2_subsystem_executor.rs`
- Tests: `tests/integration_sql_ordering.rs`, `tests/planner_indexes.rs`

### Required access-path assertions
- Required plan shape: index-backed top-N or projection-preordered bounded scan
- Forbidden plan shape: full scan followed by broad sort
- Explain/assertion coverage: assertions that ordering is satisfied by access path where contract requires it

### Query Pattern: Filtered page

### Purpose
Serve operator work queues and user-visible filtered lists.

### Shape
```sql
SELECT id, status, created_at
FROM orders_projection
WHERE tenant_id = $1
  AND status = $2
  AND created_at >= $3
ORDER BY created_at DESC
LIMIT 50;
```

### Data assumptions
- Rows/documents: 10k baseline and higher
- Cardinality: partitioned by tenant and status
- Selectivity: selective multi-predicate filters
- Indexes expected: composite or covering index, or projection materialized to the access pattern
- Projection freshness: fresh

### Performance target
- Explicit interactive paging latency target
- Throughput: stable under repeated filtered paging
- Max result size: page-sized
- Max memory per query: bounded
- Cold/warm behavior: documented

### Required execution strategy
Composite filtering path with index support or direct projection materialization.
The baseline optimized path is a composite scalar index with equality-prefix filters followed by a
single range/order field, for example `(tenant_id, status, created_at)`.

### Non-goals
Ad hoc combinations without supporting index/layout are not guaranteed to meet the contract.
Mixed-direction suffix ordering without a selective equality-prefix index remains explicit
follow-on scope.

### Validation
- Benchmarks: `perf.read_path.mixed_order.10k`, `perf.read_path.mixed_order.100k`, `perf.read_path.mixed_direction_suffix.10k`, `perf.read_path.mixed_direction_suffix.100k`, `perf.read_path.expression_index.10k`, `perf.read_path.expression_index.100k`, `perf.read_path.expression_index_range.10k`, `perf.read_path.expression_index_range.100k`, `perf.read_path.expression_index_order.10k`, and `perf.read_path.expression_index_order.100k`
- Tests: `tests/integration_sql_scalar_indexes.rs`, `tests/integration_sql_read_path_depth.rs`, `tests/planner_read_path_depth.rs`, `tests/planner_indexes.rs`

### Required access-path assertions
- Required plan shape: composite filter/order access path or equivalent projection-local shape
- Forbidden plan shape: wide tenant scan with late filter/sort
- Explain/assertion coverage: planner assertions for predicate and ordering pushdown

### Query Pattern: Count / exists

### Purpose
Serve presence checks, badge counts, queue sizes, and gating logic.

### Shape
```sql
SELECT COUNT(*) FROM orders_projection WHERE status = $1;
SELECT EXISTS(
  SELECT 1 FROM orders_projection WHERE tenant_id = $1 AND external_id = $2
);
```

### Data assumptions
- Rows/documents: 10k baseline and higher
- Cardinality: varies by filter
- Selectivity: exact lookup or bounded filtered set
- Indexes expected: scalar/composite index or aggregate acceleration where applicable
- Projection freshness: fresh or freshness explicitly documented

### Performance target
- Separate targets for `COUNT(*)`, filtered counts, and `EXISTS`
- Throughput: stable under high repetition
- Max result size: scalar
- Max memory per query: bounded and minimal
- Cold/warm behavior: documented

### Required execution strategy
Short-circuit `EXISTS`; index-aware or accelerated count path where supported.

### Non-goals
Arbitrary global exact counts over large unindexed datasets are not interactive by default.

### Validation
- Benchmarks: add dedicated count/exists cases to `benches/tier2_subsystem_executor.rs`
- Tests: `tests/integration_sql_aggregates.rs`, planner/executor coverage

### Required access-path assertions
- Required plan shape: short-circuit `EXISTS`; index-aware or accelerated count path where available
- Forbidden plan shape: full materialization for simple existence checks
- Explain/assertion coverage: executor/planner assertions for short-circuit behavior where exposed

### Query Pattern: Aggregates

### Purpose
Serve dashboards, summary panels, and grouped operational totals.

### Shape
```sql
SELECT status, COUNT(*) AS total, SUM(amount) AS amount_total
FROM orders_projection
GROUP BY status
ORDER BY status;
```

### Data assumptions
- Rows/documents: 10k baseline, then larger tiers
- Cardinality: low-to-moderate grouping cardinality
- Selectivity: optional filter before grouping
- Indexes expected: aggregate acceleration, column batch, or projection-local pre-aggregation where required
- Projection freshness: must be declared if derived acceleration is used

### Performance target
- Explicit p50/p95/p99 by grouping cardinality and scale
- Throughput: stable for dashboard polling workloads
- Max result size: bounded group count
- Max memory per query: bounded by group cardinality contract
- Cold/warm behavior: documented

### Required execution strategy
Aggregate path, column batch, rollup, or pre-projected summary.

### Non-goals
High-cardinality ad hoc aggregation is not an interactive guarantee unless materialized for that read model.

### Validation
- Benchmarks: `benches/tier2_subsystem_executor.rs`, `benches/tier3_system_query.rs`
- Tests: `tests/planner_aggregates_sets.rs`, `tests/aggregate_acceleration.rs`

### Required access-path assertions
- Required plan shape: aggregate acceleration, rollup, column batch, or explicit projection-local summary path when the contract depends on it
- Forbidden plan shape: large row-materializing aggregation for workloads declared interactive
- Explain/assertion coverage: tests must prove when accelerated paths are selected and when fallback is expected

### Query Pattern: Full-text search

### Purpose
Serve document search, operator retrieval, and keyword navigation across projections.

### Shape
```sql
SELECT id, search_score(body, $1) AS score
FROM docs_projection
WHERE search(body, $1)
ORDER BY score DESC
LIMIT 20;
```

### Data assumptions
- Rows/documents: 10k baseline, then larger tiers
- Cardinality: document corpus
- Selectivity: keyword-dependent
- Indexes expected: full-text index
- Projection freshness: freshness contract for index maintenance must be explicit

### Performance target
- Explicit top-K latency target for warm and cold paths
- Throughput: stable under concurrent search traffic
- Max result size: bounded top-K
- Max memory per query: bounded by candidate generation strategy
- Cold/warm behavior: documented separately

### Required execution strategy
Full-text index plus exact scoring of bounded candidates.

### Non-goals
Substring scanning without an inverted index is not part of the full-text contract.

### Validation
- Benchmarks: `benches/tier2_subsystem_search.rs`, `benches/tier1_hotpath_bm25.rs`
- Tests: `tests/integration_sql_fulltext_query.rs`, `tests/executor_fulltext_scoring.rs`

### Required access-path assertions
- Required plan shape: full-text index candidate generation with bounded exact scoring
- Forbidden plan shape: document-by-document substring scan
- Explain/assertion coverage: tests or explain output that confirm full-text path selection

### Query Pattern: Vector search

### Purpose
Serve nearest-neighbor retrieval for semantic lookup and AI-assisted read models.

### Shape
```sql
SELECT id, vector_distance(embedding, $1) AS distance
FROM docs_projection
ORDER BY distance ASC
LIMIT 20;
```

### Data assumptions
- Rows/documents: 10k baseline, then larger tiers
- Cardinality: embedding corpus
- Selectivity: top-K nearest neighbors
- Indexes expected: vector index when supported, otherwise explicitly degraded brute-force path
- Projection freshness: vector index freshness must be explicit

### Performance target
- Explicit top-K latency target by corpus size
- Throughput: stable under concurrent vector retrieval
- Max result size: bounded top-K
- Max memory per query: bounded by candidate strategy
- Cold/warm behavior: documented

### Required execution strategy
Vector index or documented brute-force fallback with degraded contract.

### Non-goals
Large-K or exact exhaustive vector ranking over large corpora is not interactive unless benchmarked as such.

### Validation
- Benchmarks: `benches/tier2_subsystem_vector.rs`, `benches/tier1_hotpath_vector_distance.rs`, `benches/tier1_hotpath_search_vector.rs`
- Tests: `tests/integration_sql_vector_query.rs`, `tests/executor_vector_scoring.rs`

### Required access-path assertions
- Required plan shape: vector index path where supported, otherwise explicit brute-force fallback contract
- Forbidden plan shape: accidental exhaustive ranking presented as indexed execution
- Explain/assertion coverage: tests must distinguish indexed and brute-force paths

### Query Pattern: Hybrid search

### Purpose
Serve text + semantic + structured retrieval workflows from the same projection.

### Shape
```sql
SELECT id,
       hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score
FROM docs_projection
WHERE tenant_id = $3
ORDER BY score DESC
LIMIT 20;
```

### Data assumptions
- Rows/documents: 10k baseline and higher
- Cardinality: mixed search/vector corpus
- Selectivity: bounded top-K with structured filters
- Indexes expected: full-text + vector + structured filter path, or projection materialized for the workflow
- Projection freshness: freshness of all derived structures must be explicit

### Performance target
- Explicit top-K latency target for exact final results
- Throughput: stable under concurrent hybrid retrieval
- Max result size: bounded top-K
- Max memory per query: bounded by candidate-merge strategy
- Cold/warm behavior: documented

### Required execution strategy
Bounded candidate generation plus exact final filtering/scoring. Planner must make the selected stages explainable.

### Non-goals
Unbounded merge-and-rerank over the full corpus is not an interactive contract.

### Validation
- Benchmarks: `benches/tier2_subsystem_hybrid.rs`
- Tests: `tests/integration_sql_hybrid_query.rs`, `tests/executor_hybrid_scoring.rs`

### Required access-path assertions
- Required plan shape: bounded candidate generation from text/vector paths plus exact final filter/scoring
- Forbidden plan shape: full-corpus merge/rerank for interactive patterns
- Explain/assertion coverage: explain/tests must identify candidate stages and fallback reasons

### Query Pattern: Time bucket

### Purpose
Serve time-series dashboards and operational trend reporting.

### Shape
```sql
SELECT time_bucket('1 day', created_at) AS day, COUNT(*) AS total
FROM events_projection
GROUP BY day
ORDER BY day;
```

### Data assumptions
- Rows/documents: 10k baseline and higher
- Cardinality: time-partitioned event data
- Selectivity: bounded date ranges or full series by contract
- Indexes expected: rollup, aggregate acceleration, column batch, or materialized buckets
- Projection freshness: staleness budget must be explicit

### Performance target
- Explicit latency targets per bucket count
- Throughput: stable for dashboard/reporting reads
- Max result size: bounded by bucket cardinality
- Max memory per query: bounded
- Cold/warm behavior: documented

### Required execution strategy
Time-bucket-aware aggregate path or materialized rollup with fallback semantics.
Time-series range indexes use persisted bucket-native membership when the bucket width is supported,
then load row blobs and run the normal filter/sort/projection path for correctness.
Unsupported, missing, corrupt, or transaction-local bucket metadata falls back to the row-backed scan baseline with bucket diagnostics.

### Non-goals
Arbitrary ad hoc bucket widths over large unprepared datasets are not guaranteed interactive.

### Validation
- Benchmarks: `perf.time_series.window_scan.10k`, `perf.time_series.window_scan.100k`, `perf.time_series.retention.10k`, `perf.time_series.retention.100k`, `perf.time_series.rollup_refresh.10k`, and `perf.time_series.rollup_refresh.100k`
- Tests: `tests/time_series_indexes.rs`, `tests/time_series_retention.rs`, `tests/time_series_rollups.rs`

### Required access-path assertions
- Required plan shape: rollup or bucket-aware aggregate path when the contract depends on it
- Forbidden plan shape: large raw-row scan when a declared rollup path should apply
- Explain/assertion coverage: tests must show freshness, fallback, selected rollup behavior, selected time-series index, bucket-native hits, scanned/skipped bucket counts, and retention freshness effects

### Query Pattern: Column batch

### Purpose
Serve analytical read models whose shape is intentionally scan-oriented.

### Shape
```sql
SELECT region, SUM(amount)
FROM sales_projection
WHERE created_at >= $1
GROUP BY region;
```

### Data assumptions
- Rows/documents: 10k baseline, then higher analytical tiers
- Cardinality: moderate-to-large scan set
- Selectivity: analytical filters
- Indexes expected: column batch or analytical projection layout
- Projection freshness: freshness/fallback contract must be explicit

### Performance target
- Explicit latency target for analytical scans
- Throughput: stable for concurrent reporting workloads within declared limits
- Max result size: bounded by group/output contract
- Max memory per query: bounded by batch and grouping limits
- Cold/warm behavior: documented

### Required execution strategy
Column-batch scan or projection materialized for analytical reads.

### Non-goals
Forcing row-oriented primary projections to satisfy every analytical scan interactively is not required.

### Validation
- Benchmarks: analytical subsystem or system-query bench coverage
- Tests: `tests/column_batches.rs`, `tests/aggregate_acceleration.rs`

### Required access-path assertions
- Required plan shape: column-batch or analytical projection scan
- Forbidden plan shape: row-materializing fallback for workloads whose contract depends on analytical layout, except where fallback is explicitly documented
- Explain/assertion coverage: tests must prove acceleration selection and fallback semantics

### Query Pattern: Join-like reads

### Purpose
Serve product views that combine related entities, usually from pre-shaped projections rather than heavy runtime joins.

### Shape
```sql
SELECT order_id, customer_name, total
FROM orders_with_customer_projection
WHERE tenant_id = $1
ORDER BY created_at DESC
LIMIT 50;
```

### Data assumptions
- Rows/documents: 10k baseline and higher
- Cardinality: pre-projected denormalized read model
- Selectivity: tenant/page-sized access pattern
- Indexes expected: indexes on the materialized read shape
- Projection freshness: projection freshness must be explicit

### Performance target
- Match ordered-page or filtered-page targets for the materialized read shape
- Throughput: stable for interactive list/detail use
- Max result size: bounded page/result contract
- Max memory per query: bounded
- Cold/warm behavior: documented

### Required execution strategy
Prefer projection materialization over runtime-heavy joins for latency-sensitive paths.

### Non-goals
General-purpose multi-table join optimization is not the default answer for product-critical read models.

### Validation
- Benchmarks: pattern-specific benches should use materialized read shapes where applicable
- Tests: `tests/integration_sql_joins.rs`, `tests/integration_sql_join_plans.rs`

### Required access-path assertions
- Required plan shape: projection-backed ordered/filter path for latency-sensitive reads
- Forbidden plan shape: runtime-heavy join trees for patterns declared projection-shaped
- Explain/assertion coverage: tests should distinguish projection-shaped reads from general join execution

## Benchmark Ownership

Performance contracts should map directly to the existing tiered benchmark suite:

- `tier1`: hot-path primitives that support query contracts
- `tier2`: subsystem contracts for parser, binder, planner, executor, search, vector, hybrid, ingest, and plan cache
- `tier3`: end-to-end query, concurrency, rebuild, startup, and mixed-load contracts
- `tier4`: protocol-facing contracts, with pgwire treated as the primary query interface and HTTP as secondary/admin

Where an existing benchmark does not exercise a required query pattern, add a focused benchmark rather than broadening a generic workload without clear ownership.

## Contract Governance

A query pattern is not considered optimized until:

- the pattern contract is documented
- the intended execution strategy is implemented or the projection-materialization requirement is explicit
- required and forbidden access paths are documented
- a deterministic benchmark exists for the pattern
- the owning benchmark produces useful manual p50, p95, p99, throughput, fallback, and evidence signals
- plan-shape or behavior tests protect the execution path

Unsupported patterns should be documented as non-goals rather than left as ambiguous slow paths.
