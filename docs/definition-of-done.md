# Definition of Done

Feature work is not done when code compiles. A Cassie feature is done when implementation, tests, documentation, compatibility, and operational behavior are all explicit.
Production-readiness evidence is tracked in [Production Readiness](production-readiness.md).

## Status Levels

| Status | Required Bar |
| --- | --- |
| Implemented | Code path exists and has targeted tests. Documentation and compatibility may still be incomplete. |
| Experimental | Feature is usable for supported cases, but compatibility or output shape may change; promotion requires [Experimental Promotion Criteria](experimental-promotion-criteria.md). |
| Stable | Feature has tests, documentation, known compatibility boundaries, and deterministic failure behavior. |
| Beta-ready | The documented pre-release support envelope passes the locked release gates, production-dependency audit, benchmark compilation, and disk-backed smoke evidence. Experimental capabilities remain explicitly labelled and may change. |
| Production-ready | Stable plus benchmark or operational evidence for performance-sensitive paths. |

## Required for Implemented

- Parser, binder, planner, executor, storage, and protocol/API behavior are marked Complete, Partial, or N/A.
- Tests use `should_` names and `// Arrange / Act / Assert` comments.
- Async tests use a current-thread Tokio runtime builder rather than `#[tokio::test]`.
- New source and test files stay under 1,000 lines.
- Large legacy files are split before broad feature work grows them.
- Midge remains the direct storage layer.

## Required for Stable

- All implemented behavior has focused unit or integration tests.
- Unsupported behavior returns deterministic errors where reachable.
- PostgreSQL compatibility notes list supported behavior, unsupported behavior, and intentional differences.
- User-visible SQL, API, EXPLAIN, metrics, catalog, or protocol behavior is documented.
- The feature has an owner or owning subsystem.
- The feature does not rely on hidden local configuration outside documented `CASSIE_*` variables.
- Read-model features document freshness, replay, rebuild, verification, and fallback behavior where applicable.

## Required for Production-Ready

- All Stable requirements are met.
- Performance-sensitive features have benchmark or metrics evidence.
- EXPLAIN or metrics expose important planner/executor choices when behavior may surprise users.
- Fallback behavior is deterministic and observable.
- Restart, hydration, rebuild, rename/drop, and cleanup behavior is tested when the feature persists metadata.
- Pgwire-visible behavior has SQLSTATE coverage for common failure paths.
- Projection lifecycle features have evidence for idempotent replay, failed-build isolation, swap safety, and operator diagnostics where applicable.

## Validation Order

Use this order for feature close-out:

```sh
cargo build --locked
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo fmt --all -- --check
cntryl-tools validate-tests -f <path>
```

Run `cntryl-tools validate-tests -f <path>` for every touched test file.

## Feature Checklist

Every feature record should answer:

- What is the supported scope?
- Which subsystem owns it?
- Which parser, binder, planner, executor, storage, and protocol/API pieces are complete?
- Which tests prove it?
- What PostgreSQL behavior is supported?
- What PostgreSQL behavior is unsupported?
- What differences are intentional?
- What remains before production-ready?
- What compatibility guarantee applies?
