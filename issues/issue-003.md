# Issue 003: Idempotent Replay Ingestion

Milestone: Read-Model Core
Area: Projection Lifecycle
Status: Open
Priority: P0

## Requirements

Support deterministic projection replay ingestion semantics so applying the same event stream to the same projection definition is replay-safe.

## Functional Scope

- Add an internal replay ingestion path that updates projection rows and checkpoint metadata together according to Cassie's supported durability model.
- Define duplicate event handling using source identity and event/checkpoint identifiers.
- Define out-of-order event behavior as deterministic rejection or quarantine with observable diagnostics.
- Record replay batch id, applied event count, skipped duplicate count, lag, freshness, and last error.
- Ensure restart recovery can resume from the persisted checkpoint state without corrupting projection rows.

## Non-Goals

- Do not implement a general event-store client or event subscription runtime.
- Do not implement materialized projection definitions or version swaps in this issue.
- Do not make arbitrary OLTP writes part of the replay contract.

## Acceptance Criteria

- Reapplying an already-applied event is idempotent and observable.
- Out-of-order or conflicting replay input fails or quarantines deterministically.
- Partial replay failure leaves checkpoint state and projection rows in a diagnosable state.
- Restart after replay preserves source position, lag, and last replay diagnostics.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering first replay, duplicate replay, out-of-order input, partial batch failure, restart recovery, metrics, and catalog diagnostics.
- Include result-level tests proving projection rows remain deterministic after duplicate replay.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test integration_sql_insert_values --test integration_sql_projection`
- `cargo test --locked --test midge_metadata_stats --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
