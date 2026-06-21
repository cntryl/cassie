# Phase 05 Issue 01: Write Performance Contracts

Milestone: Read-Model Write Optimization
Area: Contracts
Status: Open
Priority: P2

## Requirements

Define explicit performance contracts for Cassie's write-side read-model workflows before changing the write path.
Optimization work in phase 05 must be driven by replay, ingest, rebuild, and index-maintenance contracts rather than generic throughput claims.
Correctness is required but not sufficient: each supported write pattern must also prove that it uses the intended Midge-efficient write path.

## Dependencies

- Depends on `docs/performance-contracts.md` for the contract format and benchmark discipline.
- Depends on phase 02 issue 07 for rebuild-oriented benchmark baselines.

## Handoff

- Provides the write-side contract set consumed by the rest of phase 05.

## Functional Scope

- Define write-pattern contracts for single-row projection mutation, replay batch ingestion, duplicate replay skip, index-maintained writes, projection rebuild ingest, index rebuild/backfill, checkpoint/metadata updates, and swap-adjacent rebuild writes.
- Document data assumptions, freshness constraints, throughput/latency targets, memory budgets, and expected Midge write paths.
- Define required and forbidden write-path characteristics for each pattern.
- Map each contract to deterministic benchmark fixtures and regression thresholds.
- Identify which contracts are interactive paths and which are bulk/replay/rebuild paths.
- Define the write amplification categories used by phase 05: row writes, row deletes, index writes, index deletes, metadata writes, replay duplicate checks, batch flushes, and rebuild/version writes.

## Write Contract Template

Each write pattern must use this shape:

```md
## Write Pattern: <name>
### Purpose
What read-model lifecycle need this serves.

### Shape
API, SQL, replay batch, rebuild workflow, or DDL example.

### Data assumptions
- Rows/events:
- Batch size:
- Indexes maintained:
- Freshness/checkpoint behavior:
- Idempotency requirement:

### Performance target
- p50 latency:
- p95 latency:
- p99 latency:
- Throughput:
- Max memory per batch/query:
- Write amplification budget:
- Cold-cache behavior:
- Warm-cache behavior:

### Required write strategy
Midge row/index/metadata write path expected for this pattern.

### Forbidden write strategy
Generic or wasteful behavior that must not satisfy the contract.

### Validation
Benchmark name, fixture size, counters, and expected assertions.
```

## Initial Write Patterns

| Pattern | Benchmark ownership | Required write behavior |
| --- | --- | --- |
| Single projection mutation | `tier2_subsystem_ingest` or focused mutation bench | direct row write plus affected index and metadata deltas |
| Replay batch ingestion | `projection_write_path`, `projection_lag_catchup` | batch-local validation and grouped row/index/checkpoint writes |
| Duplicate replay skip | `projection_duplicate_replay` | duplicate detection without row or index rewrite |
| Indexed mutation | `tier2_subsystem_ingest`, index integration tests | update only affected index entries |
| Projection refresh/build | `projection_refresh`, `projection_rebuild_query` | bulk-oriented inactive target writes where applicable |
| Projection verification-adjacent rebuild | `projection_verify` | preserve hash/verification metadata compatibility |
| Version swap-adjacent writes | `projection_swap` | bounded metadata update, no data rewrite on activation |
| Index rebuild DDL | `index_rebuild_ddl` | streaming source scan plus ordered index writes |

## Implementation Plan

### Step 1: Inventory current write paths

- Read `src/app/replay.rs` and document replay batch flow: metadata load, duplicate check, row write/delete, event record, replay metadata persistence, runtime metrics.
- Read `src/midge/adapter/documents.rs` and document single-document write behavior: schema lookup, row encoding, vector index maintenance, row hash write, column-batch rebuild, projection hash refresh.
- Read `src/midge/adapter/projections.rs` and document projection metadata/event ledger keys and transaction boundaries.
- Read `src/midge/adapter/metadata.rs` and document index metadata writes, vector index rebuilds, and index delete behavior.
- Read materialized projection rebuild code in `src/executor/execution/materialized_projection.rs` before defining rebuild/write contracts.
- Record findings in `docs/performance-contracts.md` under a write-pattern section instead of adding implementation notes to source code.

### Step 2: Add write contract docs

- Add a `Write Pattern Contracts` section to `docs/performance-contracts.md`.
- For each initial write pattern, fill in purpose, shape, data assumptions, performance targets, required write strategy, forbidden write strategy, and validation.
- Keep initial latency/throughput values as measured placeholders unless benchmark data already exists.
- Tie each pattern to one owning benchmark: `tier2_subsystem_ingest` for replay/ingest, `tier3_system_rebuild` for rebuild/swap/index rebuild.

### Step 3: Define write amplification vocabulary

- Define common counters in the docs before issue 06 implements them: `row_puts`, `row_deletes`, `index_puts`, `index_deletes`, `metadata_puts`, `metadata_deletes`, `duplicate_checks`, `duplicates_skipped`, `batch_flushes`, `rebuild_target_puts`, and `activation_metadata_writes`.
- Mark counters as exact, derived, or planned depending on whether the current code can expose them.
- Define derived ratios: storage writes per applied replay event, index writes per row mutation, metadata writes per replay batch, and activation writes per swap.

### Step 4: Map tests and benchmarks

- Map replay correctness tests to `tests/projection_lifecycle.rs`; create `tests/projection_write_optimization.rs` only if the existing file approaches the 1,000-line limit.
- Map storage key/layout checks to `tests/midge_row_blob_layout.rs`, `tests/midge_metadata_stats.rs`, or a new focused storage test file if needed.
- Map runtime metric assertions to `tests/metrics_runtime.rs`.
- Map compile-only benchmark validation to `tier2_subsystem_ingest` and `tier3_system_rebuild`.

### Step 5: Close the contract issue

- Update this issue with the final contract doc links before deletion.
- Do not change write-path implementation in this issue unless a tiny fixture helper is needed to make the contracts measurable.

## Non-Goals

- Do not optimize the implementation in this issue beyond what is needed to make the contracts measurable.
- Do not define external production SLAs from local benchmark results alone.

## Acceptance Criteria

- Every supported write-side workflow has a documented contract template filled in.
- Each contract identifies the intended Midge-aware write path.
- Each contract identifies forbidden generic behavior, such as per-row catalog rediscovery, full index rebuilds for row updates, duplicate replay rewrites, or active-version data rewrites during activation.
- Each contract maps to a named benchmark or explicitly creates one as follow-up.
- The contract distinguishes interactive mutation paths from replay/rebuild bulk paths.
- Write amplification categories are defined consistently enough for later diagnostics and benchmarks.

## Required Tests

- Add docs/benchmark support only where needed.
- If reusable fixture code is added, include deterministic fixture tests in `should_` style.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update roadmap/docs references when the contract surface changes.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_ingest --no-run`
- `cargo bench --locked --bench tier3_system_rebuild --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
