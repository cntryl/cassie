# Issue 112: Compressed Column Segments

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document codec metadata and compatibility rules.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
