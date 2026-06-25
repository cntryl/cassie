# Phase 11 Issue 04: pgAdmin4 Browser Workflow Support

## Status

Open.

## Goal

Support the PostgreSQL catalog and pgwire behavior needed for pgAdmin4 to register a Cassie connection, browse schemas/tables/views/indexes/constraints, and inspect table data for supported schemas.

## Dependencies

- `issues/phase-11/issue-03.md` is complete.
- Catalog and extended-query metadata baselines are in place.

## Implementation Plan

1. Capture pgAdmin4 browser/catalog queries through a local manual run or deterministic fixture and convert the PostgreSQL-behavior gaps into Rust tests.
2. Implement missing generic catalog rows/functions needed for browser navigation when Cassie has the metadata to answer correctly.
3. Keep unsupported administrative areas deterministic and documented, including maintenance, extension management, replication, tablespaces, server logs, and broad role-management workflows.
4. Add a documented manual smoke workflow if stable pgAdmin4 automation is not practical in the default suite.
5. Update `docs/postgres-compatibility.md` with supported pgAdmin4 workflows and known limitations.

## Acceptance Criteria

- pgAdmin4 support is framed as generic PostgreSQL catalog/protocol compatibility.
- Supported browser/table-data workflows have automated Rust coverage or a documented manual smoke path.
- Unsupported admin workflows are listed in compatibility docs.
- No pgAdmin4 client detection is added.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
```

