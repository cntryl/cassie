# Integration test compilation

The `pgwire_extended` integration target has grown to roughly 106,000 source
lines including its modules and fixtures. The selected organization partitions
its existing registrations into four observable contracts:

| Cargo target | Existing test directories |
| --- | --- |
| `pgwire_extended` | Protocol, sessions, parameters, execution and lifecycle |
| `pgwire_types` | Types, type resolution and scalar functions |
| `pgwire_grammar` | SQL grammar through the wire interface |
| `pgwire_catalogs` | Catalog and routine metadata through the wire interface |

Leaf files, assertions, attributes and module aliases remain intact. Each
behavior test must have exactly one owner. Shared fixtures remain in
`tests/support/`. The `cassie-integration-tests` workspace package has a library,
`cassie_test_support`, that compiles
the common wire and SQL fixture API once; integration roots import its modules by
their established names. Specialized helpers remain ordinary source modules.
Fixture self-tests run once in the support library test executable with their
established names, rather than repeating in each consumer. Specialized helper
registrations and every behavior test retain their existing ownership. Exact-name
subprocess tests must retain their existing module paths in every executable
that owns the fixture.

This is a measured experiment, not a compilation performance claim. Keep the
existing optimization profile and compiler checks. The approved CI workflow
checks quality alongside two package-owned test jobs. Each test job compiles
every package target with `cargo test --all-targets --no-run`, then runs that
package's complete correctness suite, including doctests. The two packages cover
the full workspace. The aggregate `backend` gate requires quality and both test
jobs to succeed; each job retains its 20-minute limit. Performance sampling remains
under the documented benchmark commands; it is not run by the correctness job.
Compare
Cargo timing reports with the same job count and warm dependency artifacts,
record library compilation separately, and compare compiled test listings to
the baseline. Run the complete ordered validation after the registration bundle
is coherent. Record actual compilation and execution evidence here before
calling the organization an improvement.

The integration package owns its test executables and fixture library and depends
one way on the production Cassie package. Cassie's unit-test build has no helper
package dependency. The root and integration package are default workspace
members, preserving ordinary `cargo test --locked` and `cargo test --test NAME`.
Explicit `-p cassie` selects production-package tests only. Integration leaves
stay in place; their Cargo target paths are explicit in `tests/Cargo.toml`.

The unpublished integration package version stays aligned with Cassie because
existing version-contract assertions compare `CARGO_PKG_VERSION`. External
SQLx/Diesel fixture manifests use absolute paths based on the integration package
root. Fixture subprocess tests keep their current executable and exact names.
The inventory recognizes test-enabled libraries beneath `tests/`.

The initial helper development-dependency trial was rejected as an ownership
boundary because it coupled Cassie's unit-test compilation to shared integration
fixtures. The selected package keeps that dependency one way without adding a
custom test runner or generated target selection.

## Test compilation profiles

Production Cassie, its unit tests, dependencies and benchmark targets keep the
existing test optimization level 1. Only the integration package's fixture and
assertion code uses level 0. This is an explicit Cargo package profile boundary,
not a global downgrade. The earlier global level-0 experiment overflowed stacks
inside production folding, CTE and parser paths and was rejected. Existing deep
expression, CTE and parser witnesses and the entire suite must pass before this
narrower boundary is qualified. Stack sizes, depth limits and assertions stay
unchanged.

## Validation record — 2026-11-05

At baseline `a10ba322`, two-job all-target test compilation with warm registry
dependencies and fresh workspace artifacts took 458.84 seconds on this Mac.
The one-way package trial with all test code optimized took 443.71 seconds; one
sample does not establish a general end-to-end speedup. The selected integration
profile rebuilt all integration/fixture targets in 30.61 seconds while reusing
unchanged optimized production artifacts; that is not a cold-build comparison.
The matching fresh-workspace comparison completed in 335.07 seconds, versus
458.84 seconds for the baseline (27.0% faster in these local runs). Both used two
Cargo jobs, warm registry dependencies and rebuilt workspace artifacts, with no
competing Rust build. Verbose invocations confirm 39 production/unit/benchmark
units at level 1 and 37 integration/library units at level 0. Hosted outcomes are
retained in the closing PR for [issue #1189](https://github.com/cntryl/cassie/issues/1189);
these Mac timings are not a Linux forecast.

The selected source passed 10,360 tests with zero failures and 12 inherited
ignores. The baseline had 10,488 passing instances: exactly 128 redundant fixture
instances were removed. All 8,577 physical Rust declarations, attributes and
behavior assertions remain. The two external-client manifest path expressions
now resolve the same fixture projects from the test package directory. Compiled
wire names (3,703 including fixture names) and SQL-query names (1,847 including
fixture names) exactly match the baseline after assigning fixture self-tests to
their library executable. The cleanup subprocess test passes there.

Build, compilation of every workspace target, strict all-target/all-feature
pedantic Clippy, formatting, the Python inventory harness and inventory checks
passed. The local CI-equivalent diagnostic using the old `--all-targets` execution
command was interrupted during Tier 2 performance sampling after all Cassie
correctness tests passed; it is not recorded as a successful command.

Jev selected explicit subsystem roots and then the one-way package boundary.
Its isolation (0.58), shared-password (0.81), working-directory (0.84), dependency
coupling (0.79) and integration-stack (0.89) leads received source and executable
probes: consistent fixture imports, unchanged subprocess names, matching package
versions and external manifest paths, parallel production/unit compilation, and
the full suite including deep direct-plan, CTE and parser witnesses. Final
isolation and coverage gap judgments were 0.16 and 0.13. These are review leads;
the source/registration comparisons and executable gates are the evidence.

The first serial hosted revision completed test compilation in 8m45s, versus the
older revision still compiling when cancelled after 12m48s, but exhausted its
20-minute job budget during correctness execution after roughly seven minutes
of prior quality/build work. This prompted the package-owned jobs above. Quality
and test dependencies use separate cache keys for their dev/test profiles. No
check, test, target compilation or benchmark assertion was removed; benchmark
sampling remains a separately invoked workload.

The aggregate `backend` shell condition was exercised for all 16 combinations
of success/failure/cancelled/skipped upstream outcomes. Only success/success
passes. Package metadata confirms the two test jobs own every workspace target.
