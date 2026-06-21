# Phase 05 Issue 04: Write-Locality Key Layout

Milestone: Read-Model Write Optimization
Area: Storage Layout
Status: Open
Priority: P2

## Requirements

Align write-side key/layout choices with replay and projection-local write patterns so Cassie preserves Midge locality instead of scattering write work across avoidable key shapes.
This issue is about making Cassie's keys and write ordering match Midge strengths; it is not a storage abstraction redesign.

## Dependencies

- Depends on phase 05 issue 01 for write contracts.
- Depends on current Midge adapter/storage metadata layout.

## Handoff

- Provides key/layout guidance and implementation changes used by replay, ingest, and rebuild paths.

## Functional Scope

- Audit projection row, metadata, and index key shapes used by the write path.
- Identify avoidable locality losses caused by generic key composition or write ordering.
- Define locality expectations for row keys, projection replay metadata, source checkpoints, duplicate-event ledgers, index keys, version/build keys, and verification-adjacent metadata.
- Prefer key prefixes that group writes by projection, version/build target, index, source, and batch where that preserves existing compatibility.
- Introduce safer key/layout improvements where they preserve compatibility and correctness.
- Document any cases where layout compatibility requires migration or dual-read/write handling.

## Required Write Path

- Write ordering and key composition preserve projection-local and index-local grouping where possible.
- Replay metadata and duplicate ledgers are keyed so duplicate checks do not require broad scans.
- Rebuild/version writes can target inactive namespaces without interleaving with active read state.
- Layout changes include explicit compatibility, migration, or dual-read/write handling.

## Forbidden Write Path

- Scattered metadata keys that require broad scans for routine replay/checkpoint operations.
- Layout changes that silently orphan existing row, index, or projection metadata.
- Active-version data rewrites when a metadata-only activation is sufficient.
- New key schemes that require a second storage abstraction above Midge.

## Implementation Plan

### Step 1: Create a key-layout audit

- Add a short design section to `docs/performance-contracts.md` or a focused phase note that lists current key prefixes from `src/midge/adapter.rs`:
  - row keys
  - legacy document keys
  - projection metadata keys
  - projection event ledger keys
  - row/range/root hash keys
  - index metadata keys
  - normalized vector keys
  - column batch keys
- For each prefix, record storage family, grouping dimension, expected operation, and whether broad scans are required.

### Step 2: Add key-shape tests before layout changes

- Add storage tests for any changed key format before implementation.
- Use `tests/midge_row_blob_layout.rs`, `tests/midge_metadata_stats.rs`, or a new `tests/midge_write_layout.rs` if a focused file is cleaner.
- Add `should_key_projection_events_by_projection_source_and_event_id`.
- Add `should_keep_rebuild_version_keys_separate_from_active_projection_keys` if version/build key changes are made.
- Add restart/hydration assertions for every persisted metadata key changed by this issue.

### Step 3: Localize key helper changes

- Keep key construction inside `src/midge/adapter.rs` helper methods.
- Do not build keys ad hoc in app, executor, or tests except through public behavior assertions.
- If a key format changes, add:
  - new key helper
  - legacy read helper if needed
  - migration or cleanup path
  - tests that prove both old and new layouts hydrate correctly

### Step 4: Improve write ordering without changing format first

- Before changing persisted key formats, group batch writes by existing prefixes in issue 02/03 helpers.
- Prefer sorted writes by collection/index/document id when that preserves semantics.
- Measure whether ordering improvements are enough before introducing compatibility-sensitive format changes.

### Step 5: Introduce compatible layout changes only when needed

- For projection duplicate ledgers, prefer keys that make duplicate checks point lookups: projection/source/event id.
- For inactive rebuild/version state, prefer keys that group by projection/version/build id.
- For index rebuild writes, prefer index-local key grouping.
- Any incompatible change must include explicit migration notes and tests.

### Step 6: Diagnostics and close-out

- Add a layout label or version to diagnostics only if it helps benchmark interpretation.
- Document intentionally unchanged layouts and why they remain acceptable.

## Non-Goals

- Do not introduce a second storage abstraction.
- Do not redesign on-disk layout without explicit compatibility handling.

## Acceptance Criteria

- Write-heavy workloads show improved locality-sensitive behavior or reduced storage round trips where measured.
- Compatibility/migration behavior is explicit for any changed key shape.
- Diagnostics identify the key/layout path being exercised by benchmarks.
- The key-layout audit names any intentionally unchanged layouts and why they remain acceptable.
- Tests cover restart/hydration for changed metadata or key prefixes.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering key ordering, compatibility behavior, restart hydration, and any required migration/dual-read paths.
- Include storage-adapter tests where key layout changes are introduced.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test midge_metadata_stats --test midge_namespace_hydration --test midge_row_blob_layout --test midge_legacy_migration`
- `cargo test --locked --test integration_sql_projection --test integration_sql_catalog`
- `cargo test --locked`
- `cargo bench --locked --bench tier3_system_rebuild --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
