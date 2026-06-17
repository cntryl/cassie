# Sprint 18 - Auth, Roles, and Security Posture

Previous: [Sprint 17 - PostgreSQL Type System and Wire Encodings](sprint-17.md)  
Next: [Sprint 19 - Compatibility Matrix and CI Gate](sprint-19.md)

## Goal

Define Cassie's V1 security model for practical PostgreSQL clients and operational deployment, including authentication, role behavior, permissions posture, and clear unsupported-feature errors.

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

- Define V1 role model, default admin role, database name handling, and session identity behavior.
- Implement authentication compatible with practical PostgreSQL clients.
- Define password storage/configuration and secret-handling expectations for single-container V1.
- Define SSL/TLS posture for V1 and ensure pgwire startup negotiation behaves deterministically.
- Implement or explicitly reject `CREATE ROLE`, `DROP ROLE`, `GRANT`, `REVOKE`, row-level security, security definer functions, and privilege escalation features.
- Ensure REST admin routes use the same authorization posture or document explicit V1 differences.
- Ensure auth failures map to PostgreSQL-style errors and useful REST status codes.

## Acceptance Criteria

- Valid users can authenticate through pgwire once real protocol support lands.
- Invalid auth attempts fail deterministically.
- Session identity is visible to SQL context functions and catalog metadata where supported.
- Unsupported role and privilege features return explicit PostgreSQL-style errors.
- REST admin authorization behavior is deterministic.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/auth.rs`: role/session identity and auth behavior.
- `tests/parser.rs`: parse or reject role/privilege SQL.
- `tests/pgwire.rs`: authentication success/failure over real protocol.
- `tests/rest.rs`: REST auth behavior.
- `tests/catalog_introspection.rs`: session identity and role metadata where exposed.

## Exit Gate

This sprint is complete when auth, role posture, unsupported security features, and REST/pgwire auth behavior are covered by validator-clean tests, targeted auth tests pass, `cargo build` passes, and Clippy is clean with warnings denied.

