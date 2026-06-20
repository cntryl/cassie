# Issue 115: Bucket Functions

Milestone: V4 - Analytical Overlay
Area: Time Series
Status: Open
Priority: P3

## Requirement

Add deterministic time bucket scalar functions for grouping timestamps into fixed-width windows.

## Functional Scope

- Support `time_bucket(width, timestamp[, origin])`, where `width` is a positive duration literal/string accepted by the binder and `origin` defaults to the Unix epoch.
- Return the inclusive bucket-start timestamp in UTC using deterministic arithmetic.
- Reject zero, negative, malformed, calendar-dependent, or overflowing widths with clear errors.
- Allow bucket functions in SELECT, GROUP BY, ORDER BY, HAVING, and time-series index planning.
- Preserve null propagation and type errors consistent with other scalar functions.

## Non-Goals

- Do not implement timezone calendars, month-length buckets, or business-calendar logic.
- Do not implement rollup storage here; that is issue 116.

## Acceptance Criteria

- Bucket function output is deterministic for boundary, before-origin, after-origin, null, and overflow cases.
- GROUP BY and ORDER BY over bucket expressions produce correct rows.
- Invalid widths and argument types fail during binding/execution with stable errors.
- Function behavior is available through pgwire and REST SQL paths.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering fixed windows, custom origin, boundary timestamps, nulls, invalid widths, GROUP BY/HAVING, and pgwire/SQL execution.
- Include scalar function and integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document supported duration literals and timezone assumptions.

## Validation

- `cargo test --test scalar_functions --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/scalar_functions.rs`
