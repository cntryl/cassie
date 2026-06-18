# Sprint 14 - Row Storage Rebuild and Decode Controls

Previous: [Sprint 13 - Row Blob Persistence Core](sprint-13.md)
Next: [Sprint 15 - SQL INSERT VALUES](sprint-15.md)

## Goal

Add explicit row iteration, rebuild, and selective decode controls so derived indexes and future vectorized execution can consume row blobs without relying on full JSON document materialization.

## Requirements

- Add storage APIs for iterating authoritative row blobs by collection with stable row IDs.
- Add decode controls for full-row decode and projected-field decode using row-schema field IDs.
- Provide rebuild helpers that expose row data for secondary, full-text, and vector index rebuilds without changing current derived-index behavior.
- Keep legacy `doc:` compatibility behind the same iteration surface until a later migration removes legacy reads.

## Acceptance Criteria

- Row iteration returns all authoritative rows from row blobs and legacy compatible rows without duplicates.
- Projected decode returns only requested active fields and ignores retired fields.
- Rebuild helpers can feed existing full-text/vector index construction tests without changing query results.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Storage tests for full-row and projected-field decode.
- Rebuild helper tests for row blob and legacy-compatible sources.
- Regression tests for existing scans, search, vector, and restart behavior.

## Exit Gate

This sprint is complete when row iteration and decode controls are validator-clean, targeted storage/search/vector tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
