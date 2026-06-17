# Sprint 17 - PostgreSQL Type System and Wire Encodings

Previous: [Sprint 16 - PostgreSQL Catalog and Introspection](sprint-16.md)  
Next: [Sprint 18 - Auth, Roles, and Security Posture](sprint-18.md)

## Goal

Define and implement Cassie's V1 PostgreSQL type compatibility layer so SQL parsing, execution, catalog introspection, and wire encoding agree on values, OIDs, casts, nulls, and client-visible metadata.

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

- Define V1 PostgreSQL-compatible type catalog entries and OIDs for Cassie scalar, text, boolean, integer, float, numeric-compatible, JSON, UUID, timestamp/date/time, vector, null, and limited array values.
- Implement deterministic casts through `CAST(...)` and `::type` for supported V1 types.
- Define text and binary wire encoding policy for each supported type.
- Ensure row descriptions expose correct OIDs, widths, format codes, and nullability where available.
- Ensure REST JSON values and SQL values round-trip predictably.
- Return explicit unsupported errors for unsupported type families, collations, domains, ranges, enums, composite types, and advanced array behavior unless implemented.
- Align catalog introspection type rows with pgwire row metadata.

## Acceptance Criteria

- Supported types parse, bind, execute, cast, store, retrieve, and encode deterministically.
- Row description metadata matches catalog type metadata.
- Null handling is stable across SQL, REST, and pgwire.
- Unsupported types and casts fail deterministically.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/types.rs`: type conversion, casts, nulls, arrays, JSON, UUID, time values, and vector metadata.
- `tests/integration_sql.rs`: type round trips through Midge-backed SQL.
- `tests/catalog_introspection.rs`: type catalog rows and OIDs.
- `tests/pgwire.rs`: row descriptions and wire encodings after real protocol support lands.
- `tests/rest.rs`: REST payloads map to the same Cassie values as SQL writes.

## Exit Gate

This sprint is complete when type behavior and metadata are covered by validator-clean tests, targeted type tests pass, `cargo build` passes, and Clippy is clean with warnings denied.

