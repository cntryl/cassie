# Phase 11 Issue 03: Prepared Statement And Parameter Metadata Depth

## Status

Open.

## Goal

Deepen PostgreSQL extended-query compatibility for drivers and ORMs that rely on prepared statement description, parameter metadata, row descriptions, and stable SQLSTATE behavior.

## Dependencies

- `issues/phase-11/issue-02.md` is complete.
- Existing pgwire startup, simple query, extended query, prepared statement, and transaction tests remain green.

## Implementation Plan

1. Add failing pgwire tests for extended-query `Parse`, `Bind`, `Describe`, `Execute`, `Close`, and `Sync` flows used by parameterized ORM queries.
2. Cover:
   - parameter count and type metadata for explicitly typed and inferable parameters
   - row descriptions for prepared `SELECT`, `INSERT ... RETURNING`, `UPDATE ... RETURNING`, and `DELETE ... RETURNING`
   - unnamed and named statement lifecycle reuse
   - deterministic error response fields and ready-for-query state after statement errors
3. Reuse existing parser, binder, and type metadata where possible; do not introduce client-specific protocol paths.
4. Update `docs/postgres-compatibility.md` with the improved protocol behavior and remaining unsupported protocol features.

## Acceptance Criteria

- Extended-query tests cover parameterized CRUD and selected metadata edge cases.
- Existing `tokio-postgres`, psql opt-in, and SQLAlchemy opt-in probes remain compatible.
- Unsupported metadata requests fail predictably without corrupting the protocol state.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
```

