# Sprint 09 - UDFs and Stored Procedures

Previous: [Sprint 08 - Indexes and Constraints](../sprint-08.md)  
Next: [Sprint 10 - Full-Text Search Stack](../sprint-10.md)

## Goal

Add V1 programmability through SQL-defined user-defined functions and stored procedures while keeping execution deterministic, catalog-backed, and safe for a single-container Cassie runtime.

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

- Support `CREATE FUNCTION`, `DROP FUNCTION`, and SQL-bodied scalar UDFs for V1 expression contexts.
- Support `CREATE PROCEDURE`, `DROP PROCEDURE`, and `CALL` for SQL-bodied stored procedures.
- Persist UDF and procedure definitions, signatures, argument names, argument types, return types, and volatility metadata in `cf0`.
- Bind and type-check UDF and procedure calls before execution.
- Allow UDFs in projections, filters, order expressions, CTEs, and search/vector/hybrid expressions where return types are compatible.
- Allow stored procedures to invoke supported Cassie SQL operations only.
- Return explicit PostgreSQL-style unsupported errors for PL/pgSQL, native extensions, unsafe host-language UDFs, transaction-control statements inside procedures, cursors, triggers, and security definer behavior unless implemented intentionally.
- Prevent recursive function/procedure calls from causing unbounded execution.
- Ensure pgwire can expose function and procedure catalog metadata to practical clients.

## Acceptance Criteria

- SQL-bodied scalar UDFs can be created, called, persisted, dropped, and rehydrated after restart.
- Stored procedures can be created, called with `CALL`, persisted, dropped, and rehydrated after restart.
- UDFs work in supported expression positions.
- Procedure calls have deterministic command completion and error behavior.
- Unsupported language/runtime/procedure features return explicit PostgreSQL-style errors.
- Recursive or cyclic calls are rejected or bounded deterministically.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse `CREATE FUNCTION`, `DROP FUNCTION`, `CREATE PROCEDURE`, `DROP PROCEDURE`, and `CALL`.
- `tests/planner.rs`: plan function/procedure catalog operations and calls.
- `tests/executor.rs`: execute UDFs in expressions and procedures through `CALL`.
- `tests/integration_sql.rs`: persist and reload UDF/procedure definitions after restart.
- `tests/pgwire.rs`: function and procedure calls through simple and extended protocol after pgwire support lands.
- Add explicit tests for unsupported PL/pgSQL/native procedure bodies and transaction-control statements.

## Exit Gate

This sprint is complete when UDFs and stored procedures are covered by validator-clean tests, targeted programmability tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
