# Issue 117: Retention Policies

Milestone: V4 - Analytical Overlay
Area: Time Series
Status: Open
Priority: P3

## Requirement

Allow collections to declare deterministic time-based retention policies that remove expired projection rows and associated index/materialized state.

## Functional Scope

- Add catalog metadata for retention policy: collection, timestamp field, retention duration, enforcement mode, last enforcement timestamp, and state.
- Provide a SQL/admin path to create, alter, drop, and inspect retention policies.
- Enforce retention through an explicit deterministic maintenance operation and optional startup/background scheduling only when configured.
- Delete expired row blobs and all associated scalar, full-text, vector, time-series, column, and materialized projection entries atomically per row where existing write guarantees allow.
- Report deleted rows, skipped rows, errors, and lag through metrics and catalog/introspection.

## Non-Goals

- Do not implement legal hold, archive storage, or cross-instance distributed retention in this issue.
- Do not silently delete rows when timestamp values are missing or invalid; use configured behavior or skip with diagnostics.

## Acceptance Criteria

- Retention policies persist, hydrate, alter, and drop correctly.
- Explicit enforcement deletes only rows older than the configured cutoff and removes dependent index/projection state.
- Enforcement is idempotent and safe to retry after partial failure.
- Queries after enforcement do not return expired rows.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering policy create/alter/drop, explicit enforcement, missing timestamp behavior, index cleanup, rollup/materialized cleanup if present, restart hydration, and idempotent retry.
- Include scalar/integration tests and catalog assertions.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document retention syntax, enforcement mode, and operational risks.

## Validation

- `cargo test --test scalar_functions --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/scalar_functions.rs`
