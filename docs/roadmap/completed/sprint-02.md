# Sprint 02 - Midge Storage Contract and Catalog Hydration

Previous: [Sprint 01 - Foundation, Repo Contract, Runtime Baseline](sprint-01.md)  
Next: [Sprint 03 - SQL Parser and Binder V1](../sprint-03.md)

## Goal

Make Midge the durable source of truth for Cassie V1 and lock the column-family layout so schema, data, and temp state remain deterministic across startup, writes, scans, and restart recovery.

## Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` documents/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## Requirements

- Harden named family bootstrap for `cf0`, `cf1`, and `cf2`.
- Resolve families by name, not by assumed numeric IDs.
- Keep schema, collection config, index metadata, and catalog hydration data in `cf0`.
- Keep user documents and row payloads in `cf1`.
- Keep temporary execution state and scratch data in `cf2`.
- Treat `default` as engine-reserved and keep Cassie state out of it.
- Clear `cf2` during startup.
- Hydrate the in-memory catalog from `cf0` on startup.
- Persist and reload collection schemas and vector index metadata through Midge.
- Keep all storage operations inside `src/midge/adapter.rs` as a thin Midge translation boundary.

## Acceptance Criteria

- Cold boot creates required families.
- Repeated bootstrap is idempotent.
- Restart preserves collection schemas, vector index metadata, and documents.
- Family routing tests prove metadata/data/temp separation.
- No Cassie keys appear in `default`.
- Temp state is removed on startup.
- Catalog hydration reads persisted metadata from `cf0`.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/midge_cf_layout.rs`: cold boot creates `cf0`, `cf1`, and `cf2`.
- `tests/midge_cf_layout.rs`: repeat startup preserves family identity and state.
- `tests/midge_cf_layout.rs`: schema writes route to `cf0`, document writes route to `cf1`, temp writes route to `cf2`.
- `tests/midge_cf_layout.rs`: startup clears `cf2`.
- `tests/midge_cf_layout.rs`: Cassie metadata is absent from `default`.
- `tests/vector_index_metadata.rs`: vector index metadata survives restart.
- `tests/midge_error_paths.rs`: Midge errors map into deterministic `CassieError` variants.

## Exit Gate

This sprint is complete when Midge family routing, restart recovery, catalog hydration, temp cleanup, and error mapping are covered by validator-clean tests, all storage-focused tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
