# Sprint 38 - SQL Type Coverage and Metadata Fidelity

Previous: [Sprint 37 - Common Scalar Functions](sprint-37.md)
Next: [Sprint 39 - Schema DDL Breadth and Index Variants](sprint-39.md)

## Goal

Broaden Cassie's SQL type system so common table-definition and client-metadata expectations round-trip cleanly through SQL, REST, and pgwire.

## Requirements

- Add support for common type aliases such as `smallint`, `integer`, `bigint`, `int2`, `int4`, and `int8`.
- Add support for length-bearing string types such as `char(n)` and `varchar(n)`.
- Preserve type metadata through catalog hydration, `information_schema`, `pg_catalog`, REST collection definitions, and pgwire type reporting.
- Keep current numeric and date/time types stable while preserving deterministic errors for unsupported modifiers.
- Ensure type parsing stays consistent across SQL DDL and REST schema creation.

## Acceptance Criteria

- The new type names can be used in `CREATE TABLE` and REST collection schema definitions.
- Catalog introspection and pgwire metadata report the same supported type names that SQL accepts.
- Unsupported type modifiers fail deterministically instead of silently coercing.
- Existing vector, JSON, UUID, timestamp, and array types continue to round-trip.
- The sprint exits with touched-test validation, `cargo build`, and Clippy green.

## Tests

- Parser tests for the new SQL type forms and aliases.
- Schema/catalog tests for introspection round-trips.
- REST collection tests for type parsing.
- Compatibility tests for pgwire type metadata exposure.

## Exit Gate

This sprint is complete when the type coverage suite is green, touched tests validate, `cargo build` passes, and Clippy is clean with warnings denied.
