# Sprint 13 - Row Blob Persistence Core

Previous: [Sprint 12 - Runtime Observability, Plan Cache, and Operational Controls](sprint-12.md)
Next: [Sprint 14 - Row Storage Rebuild and Decode Controls](sprint-14.md)

## Goal

Make compact row blobs the authoritative V1 row format in Midge `cf1`, while preserving existing REST, SQL read, search, and vector behavior through decode compatibility.

## Requirements

- Land `docs/storage-design.md` as the V1 row storage design.
- Encode and decode row blobs with format version, schema version, flags, field count, sorted field IDs, type tags, and typed values.
- Persist row-schema metadata in `cf0`, including immutable `field_id`, `schema_version`, `next_field_id`, and retired-field state.
- Store new rows in `cf1` under `r/{collection}/{row_id}` and keep field names out of row blob bytes.
- Decode row blobs back into the existing `DocumentRef`/JSON-facing API so current executor, REST, full-text, vector, and hybrid paths keep working.
- Preserve upgrade compatibility for legacy `doc:{collection}:{id}` keys: point reads, scans, overwrites, deletes, renames, and drops must not hide, strand, or resurrect data.

## Acceptance Criteria

- Fresh writes create row blobs in `cf1`.
- Existing legacy `doc:` rows remain readable and scannable until rewritten or removed.
- Dropped fields become retired row-schema metadata and field IDs are never reused.
- Existing query, REST, search, vector, and restart tests continue passing.
- Full `cargo test`, `cargo build`, Clippy, and touched-test validation pass.

## Tests

- `tests/midge_cf_layout.rs`: row blob routing, no field names in row blobs, schema version and retired field IDs.
- `tests/midge_cf_layout.rs`: legacy `doc:` scan, overwrite cleanup, rename movement, and drop cleanup.
- Row blob module tests: sparse decode and retired field ID behavior.
- Existing executor, integration SQL, REST, metrics, plan cache, search/vector, and restart suites for regression coverage.

## Exit Gate

This sprint is complete when row-blob persistence and legacy compatibility are covered by validator-clean tests, full `cargo test` passes, `cargo build` passes, and Clippy is clean with warnings denied. When green, move this file to `docs/roadmap/completed/sprint-13.md` and update roadmap links.
