# Read-Model Autopilot Plan

This plan turns the read-model gap analysis into an ordered execution path. Autopilot work should proceed in small, validated slices and should not start broad feature implementation until the relevant issue has clear acceptance criteria and validation commands.

## Operating Rules

- Keep Cassie framed as an event-sourced read-model database.
- Treat PostgreSQL compatibility as client access and tooling support, not OLTP parity.
- Prioritize capabilities by read-model need, not by whether they resemble OLTP, OLAP, search, vector retrieval, or time-series features.
- Keep Midge as the only storage layer.
- Preserve row blobs as the correctness fallback.
- Keep source and test files under 1,000 lines; extract focused modules before adding broad behavior to near-limit files.
- Use TDD for feature work: failing `should_` test, smallest passing change, focused refactor.
- Use current-thread Tokio runtime builder tests, never `#[tokio::test]`.
- Run validation in this order: `cargo build --locked`, targeted tests, `cargo test --locked`, `cargo fmt --all -- --check`, then `cntryl-tools validate-tests -f <path>` for touched test files.

## Phase 0: Preflight

- Record `git status --short` before every slice and avoid reverting unrelated user work.
- Run the file-size audit before touching parser, binder, planner, executor, runtime, catalog, Midge, or integration tests.
- Convert existing async-test style violations only in files that the slice must touch.
- Prefer new focused modules and focused test files over growing large legacy files.

## Phase 1: Product Alignment

- Make the read-model mission the first product frame in docs.
- Move projection lifecycle, replay safety, rebuild verification, and operations visibility ahead of PostgreSQL compatibility in roadmap ordering.
- Document SQL, DML, transactions, and pgwire as projection access/mutation surfaces.
- Record intentionally unsupported PostgreSQL behavior as out of scope when it does not serve read-model workflows.

Done when:

- `docs/README.md`, `docs/product-roadmap.md`, `docs/feature-support.md`, and `docs/postgres-compatibility.md` consistently describe Cassie as an event-sourced read-model database.
- The issue backlog has explicit P0/P1 items for projection checkpoints, replay ingestion, materialization/versioning/swaps, verification, operations views, and rebuild performance targets.

## Phase 2: Projection Checkpoints And Replay

- Define durable projection source metadata: projection id, source identity, source checkpoint, last event id, replay batch id, lag, freshness, and last error.
- Persist and hydrate checkpoint metadata through existing Midge/catalog patterns.
- Expose checkpoint state through catalog/admin diagnostics and metrics.
- Define idempotent replay behavior for duplicate events, out-of-order events, partial batches, and restart recovery.

Done when:

- A projection can report which event-stream source position it represents.
- Reapplying an already-applied replay event is deterministic and observable.
- Out-of-order or conflicting replay input fails or quarantines deterministically according to the issue spec.

## Phase 3: Materialized Projection Lifecycle

- Add deterministic materialized projection create/build/refresh/drop behavior.
- Add projection versions so a new build can coexist with the active read model.
- Route normal reads to exactly one active version.
- Add verified active-version swaps with cache invalidation and rollback-capable retired versions.
- Reject DML against materialized projection outputs unless a future issue explicitly supports writable projections.

Done when:

- A failed build leaves the previous active projection readable.
- A successful swap changes future reads atomically from the reader perspective.
- Restart hydration preserves active/building/failed/retired version state.

## Phase 4: Verification

- Implement deterministic row hashes first.
- Add range hashes and projection Merkle roots after row hashes are stable.
- Add rebuild verification before marking rebuilt projections or indexes verified/safe to activate.
- Add local projection integrity verification reports after rebuild verification exists.

Done when:

- Identical logical projection rows produce identical hashes across restarts.
- Rebuilt projection versions can be verified before swap.
- Verification failure leaves the previous active projection or index usable.
- Operators can inspect mismatch, stale, missing, and unverifiable states.

## Phase 5: Operations And Performance

- Add projection-centric catalog/admin views for active version, source checkpoint, lag, freshness, rebuild state, verification state, last replay batch, last error, and fallback counters.
- Update EXPLAIN and metrics when a query reads a projection version, rollup, column batch, aggregate acceleration path, or fallback path.
- Add benchmarks for replay ingestion throughput, duplicate handling, projection rebuild, rebuild verification, version swap latency, and lag catch-up.

Done when:

- Operators can answer which projection version is being served, from which source position, with what freshness and verification state.
- Benchmark targets exist for at least the 10k-row/event path and are documented before claiming production readiness.

## Stop Conditions

Pause autopilot and ask for direction when:

- A required feature decision changes persistent metadata shape in a way that is not covered by an issue.
- Existing dirty work in a touched file conflicts with the planned change.
- A validation failure appears unrelated to the slice and cannot be isolated without broad refactoring.
- A file would exceed 1,000 lines without an extraction that is outside the slice.
