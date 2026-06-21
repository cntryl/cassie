# Issue 112: Compressed Column Segments

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirements

Compress column-batch segments with versioned codecs that preserve exact value reconstruction and row alignment.

## Functional Scope

- Add codec metadata per column segment, including codec name, version, uncompressed length, value count, null bitmap encoding, and checksum/hash where available.
- Support an uncompressed codec plus at least one value-aware codec suitable for common analytical data, such as dictionary/RLE for repeated values or delta encoding for numeric/timestamp values.
- Select codecs deterministically during batch build/rebuild and allow fallback to uncompressed when compression is ineffective or unsupported.
- Decode compressed segments for scan and aggregate paths with the same type/null/sparse behavior as uncompressed column batches.
- Report compressed bytes, uncompressed bytes, codec choice, and decode fallback through metrics.

## Non-Goals

- Do not change row blob encoding.
- Do not require every type to have a specialized codec in the first implementation.

## Acceptance Criteria

- Compressed and uncompressed segment reads return identical values and row-id ordering.
- Restart, rebuild, and mixed codec-version hydration work without data loss.
- Corrupt or unsupported compressed segments fail clearly or fall back to row blobs without silent incorrect results.
- Metrics demonstrate compression ratio and decode usage.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering codec selection, round-trip decode, null/sparse fields, unsupported codec fallback, restart hydration, rebuild, and corruption handling.
- Include planner/integration coverage for compressed column scans.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module_organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test parser_indexes --test parser_cte_schema`
- `cargo test --locked --test planner_logical --test planner_physical --test planner_commands`
- `cargo test --locked --test integration_sql_projection --test integration_sql_aggregates --test integration_sql_ordering --test integration_sql_catalog`
- `cargo test --locked --test midge_metadata_stats --test midge_row_blob_layout --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
