# Phase 11 Issue 05: Opt-In ORM And Tooling Smoke Probes

## Status

Open.

## Goal

Add isolated opt-in smoke probes for representative ORM and tooling workflows while keeping the default test suite deterministic and dependency-light.

## Dependencies

- `issues/phase-11/issue-04.md` is complete.
- Catalog, migration DDL, prepared metadata, and pgAdmin4 browser support have documented behavior.

## Implementation Plan

1. Add opt-in probes behind environment variables following existing psql and SQLAlchemy patterns.
2. Prioritize:
   - Prisma CLI smoke for `db pull` or the smallest deterministic introspection workflow that does not commit generated artifacts.
   - A pgAdmin4 manual or automated smoke path, depending on whether a stable local dependency path exists.
   - One additional lightweight driver/tool probe only if it is deterministic and does not add default-suite brittleness.
3. Keep full official ORM/toolkit suites external.
4. Document exact local commands, required binaries/packages, env vars, and expected limitations.
5. Ensure default `cargo test --locked` skips probes unless their explicit env vars are set.

## Acceptance Criteria

- Default tests remain dependency-light and deterministic.
- Opt-in probes start Cassie over pgwire and use PostgreSQL-compatible connection behavior.
- Probe docs identify failures as PostgreSQL compatibility gaps, not client-specific workarounds.
- `docs/postgres-compatibility.md` reflects smoke status and limitations.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
```

