# Issue 111: Column Batches

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirement

Add optional column-batch index storage so analytical scans can read selected columns without decoding full row blobs.

## Functional Scope

- Support a SQL/catalog path for creating a column index over explicit fields, using row blobs as the source of truth.
- Store versioned column-batch metadata: collection, index name, schema epoch, fields, segment size, row-id range, null bitmap availability, and encoding version.
- Build column batches from row blobs and maintain them for SQL/REST writes, updates, deletes, rebuilds, collection rename/drop, and startup hydration.
- Preserve row-id mapping so column-batch reads can be reconciled with row blobs for fallback and correctness checks.
- Expose column-batch presence and use through catalog introspection, EXPLAIN, and metrics.

## Non-Goals

- Do not make column batches the default storage format.
- Do not require compression in this issue; compressed segments are issue 112.
- Do not support full column-store tables here; that is issue 131.

## Acceptance Criteria

- Column-batch metadata and payloads are created, persisted, hydrated, rebuilt, and dropped without changing row-blob contents.
- Column batches preserve nulls, missing sparse fields, type fidelity, and deterministic row order.
- Executor can read a column batch for covered analytical scans and fall back to row blobs when a batch is missing or incompatible.
- EXPLAIN/metrics identify column-batch scans and row-blob fallback.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering creation, persistence, hydration, rebuild, write maintenance, null/sparse fields, rename/drop cleanup, and fallback.
- Include parser/planner/integration tests for the chosen column-index syntax.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches catalog/storage/planner/executor.
- Run `cargo fmt --all -- --check`.
- Document column-index syntax and storage invariants.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
