# Sprint 01 - Foundation, Repo Contract, Runtime Baseline

Previous: [Roadmap README](README.md)  
Next: [Sprint 02 - Midge Storage Contract and Catalog Hydration](sprint-02.md)

## Goal

Establish the repository contract and runtime baseline for Cassie V1. This sprint makes startup behavior, error mapping, test conventions, and core runtime expectations explicit before deeper storage, query, or protocol work continues.

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

- Treat `AGENTS.md` as the authoritative engineering contract for Cassie test shape and TDD workflow.
- Document the core runtime expectations for `Cassie`, `CassieSession`, `CassieError`, startup, shutdown, health, and config loading.
- Ensure `Cassie::startup()` is idempotent and can be called repeatedly without corrupting storage state or catalog state.
- Ensure startup failures map into deterministic `CassieError` variants with actionable messages.
- Keep integration tests at the repository root in `tests/`.
- Keep runtime startup compatible with a single-process, single-container V1 deployment.
- Do not introduce service registries, distributed lifecycle hooks, or multi-node assumptions.

## Acceptance Criteria

- `Cassie::startup()` can be called repeatedly without corrupting state.
- Startup failure maps to `CassieError`.
- `AGENTS.md` conventions are reflected in this roadmap and are followed by touched tests.
- Root-level integration test structure remains canonical.
- `cargo test --test midge_cf_layout --test parser --test planner --test executor` passes.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- Add or confirm a single-behavior startup idempotence test.
- Add or confirm a single-behavior startup failure mapping test.
- Add or confirm a health/config runtime behavior test if implementation changes touch those surfaces.
- Validate every edited test file with `cntryl-tools validate-tests -f <file>`.
- Run targeted runtime and query-stack tests before closing the sprint.

## Exit Gate

This sprint is complete when runtime startup is deterministic, failure behavior is explicit, touched tests are validator-clean, targeted tests are green, `cargo build` passes, and Clippy is clean with warnings denied.
