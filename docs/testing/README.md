# Test organization and inventory

Every test under `tests/` belongs to an existing Cargo suite and a focused behavior module. Use the observable contract to choose its directory. Keep issue numbers, PostgreSQL versions, authoring waves and completion status in inventory metadata and test comments.

This inventory covers the entire current `tests/` tree, including untracked backfill files, shared support, external-client fixtures, corpus data and the Python parity harness. Unit tests in `src/` and benchmark definitions in `benches/` are outside this inventory.

## Start here

| Record | Contents |
| --- | --- |
| [Summary and owner index](summary.md) | Current counts, every owner page, Cargo registrations, files over their size limit |
| [Every existing file](files.md) | Current file, role, size, test count, registered suites, selected destinations and migration action |
| [Reconciled backfill tests](planned-tests.md) | Each inventory `should_…_given_…_when_…` name, issue, actual assertion and authoring/execution status |
| [Machine-readable inventory](inventory.json) | Every existing declaration, source location, full registered test identities and selected placement, plus every reconciled backfill item |
| [Editable backfill list](planned-tests.json) | The editable issue-to-assertion ledger |
| [Owner assertion anchors](owner-assertions.md) | Exact finite test links for all 69 open owners |
| [Organization validation](validation.md) | Checks run for this pass, observed prerequisite failures and incomplete gates |

The per-owner pages linked from the summary contain a row for every existing test. Each row names both its current file and its selected destination file. The file inventory also accounts for files without test bodies.

Counts refer to test declarations. A declaration can exercise several SQL cases; this census does not expand every table-driven assertion into a separate case.

## Directory structure

```text
tests/
  <existing_suite>.rs             Cargo entry point and module registrations
  <existing_suite>/
    <behavior>/
      <focused_contract>.rs      Tests for that behavior
  support/
    <focused_fixture>.rs         Reusable fixtures, guards and assertions
  compatibility/
    lifecycle/<object>/          Cross-session and object lifecycle contracts
    clients/<client>/            Named-client qualification
    extensions/<extension>/      Explicit extension qualification
  corpus/                        Oracle cases and immutable qualification data
  fixtures/                      External-client projects and captured traces
  pg18_parity/                   Python harness and its tests
```

Nested modules stay inside the existing suite. They do not become additional Cargo targets. The current target names and CI shard assignments are listed in the summary.

## Ownership rules

Choose the suite from the assertion being made. A SQL statement can appear in several suites when each test observes a different contract.

| Contract under test | Owner and example destination |
| --- | --- |
| Parser acceptance, rejection, precedence or AST shape | `parser_types/grammar/<feature>/<contract>.rs` |
| SQL values, bags/order, NULL behavior, expressions or grouping | `sql_queries/<feature>/<contract>.rs` |
| SQL mutation effects, RETURNING or conflict behavior | `sql_mutations/<feature>/<contract>.rs` |
| Parse/Describe/Bind/Execute, OIDs/typmods, SQLSTATE, text/binary codecs or cancellation | `pgwire_extended/<feature>/<contract>.rs` |
| Connection startup, authentication or basic protocol state | `pgwire_core/<feature>/<contract>.rs` |
| Catalog visibility, lookup or permissions | `catalog_security/catalogs/<object>/<contract>.rs` or `catalog_security/security/<contract>.rs` |
| Plans, planner controls, estimates or cache invalidation | `planner_controls/<feature>/<contract>.rs` |
| Physical executor behavior, parallelism or execution resources | `executor/<feature>/<contract>.rs` |
| Storage/index paths, recovery or scan equivalence | `storage_indexes/<feature>/<contract>.rs` |
| Session isolation, transactional DDL, object lifecycle or coordinated sessions | `compatibility/lifecycle/<object>/<contract>.rs` |
| Named clients, dump/restore or explicit extension qualification | `compatibility/clients/<client>/…` or `compatibility/extensions/<extension>/…` |
| A deliberately cross-surface relational qualification contract | `relational_qualification/<feature>/<contract>.rs` |
| Analytics, REST, search, vectors, domain models, metrics or scan controls | The matching existing suite and its behavior directory |
| Configuration tests requiring their own test process | Keep the `configuration_isolated` Cargo target |
| Existing narrowly scoped standalone SQL/regression targets | Keep the existing target; organize leaves beneath its matching directory |

For example, scalar-subquery row values belong under `sql_queries/subqueries/`; their wire cardinality SQLSTATE belongs under `pgwire_extended/subqueries/`. Array aggregates belong under `aggregates/arrays/`; array binary framing belongs under `types/arrays/`. SQL-defined aggregate dependencies belong under `compatibility/lifecycle/aggregates/`.

`layout-decisions.json` records the reviewed physical locations. Existing module aliases preserve established test names when files move. Shared support, external clients, corpus data and the Python harness retain their distinct boundaries. New placements require checking the actual assertion and helper ownership before updating that manifest.

## Naming and file size

- Use behavior names for permanent directories and files: `subqueries/cardinality.rs`, `types/numeric/binary.rs`, `aggregates/ordered_set/percentiles.rs`, `lifecycle/operators/restart.rs`.
- Give new tests `should_<outcome>_given_<conditions>_when_<action>` names and Arrange/Act/Assert sections. Preserve existing test identities during organization.
- Keep Rust leaves and support files below 1,000 lines. The repository permits flat Cargo suite files up to 12,000 lines; keep these as registration roots with focused leaves.
- Review groups with more than 800 lines of test bodies before moving them. Imports and fixtures also consume the 1,000 line allowance.
- Separate independent contracts when a file mixes values, metadata, errors, resources and restart behavior. Keep small cohesive modules together when splitting would obscure shared setup.
- Put reusable setup in `tests/support/`. Keep scenario tests in their owning suite, even when they use shared fixtures.

## Preserve these boundaries during cleanup

1. Preserve Cargo target names and shard membership. Register new leaves in an existing suite and verify `backend_ci`.
2. Preserve fully qualified existing test names and exact-name subprocess callers. File moves can use a module alias via `#[path]` when needed to retain an established namespace; ordinary nested modules can use Rust's default file resolution.
3. Preserve test attributes, assertions, fixture scope, storage-mode selection and process isolation. Do not change expected results as part of a file move.
4. Keep external-client Cargo projects, Python runtime boundaries and corpus paths intact until their callers and manifests are updated together.
5. Record execution evidence separately. An authored or registered test is not a demonstrated failure or pass. Shared support declarations may appear in multiple test binaries.
6. Deduplicate proposed backfill contracts against existing assertions before writing them. The inventory-name count includes linked existing assertions and oracle-corrected proposals.

## Migration sequence and parallel ownership

Use one working branch and checkout. Give agents exclusive file sets; the integrator owns shared suite registrations and this generator.

| Slice | Exclusive files | First cleanup |
| --- | --- | --- |
| SQL and query behavior | `parser_types/`, `sql_queries/`, `sql_mutations/`, query-oriented wire leaves | Split mixed grammar/query wave files; extract focused modules from suite roots |
| Types, functions, aggregates and catalogs | Type/function/aggregate wire and query leaves; `catalog_security/` | Separate values from metadata/codecs; split oversized numeric, temporal, array and JSON files |
| Transactions, lifecycle and qualification | `compatibility/`, process-isolated cases and qualification fixtures | Group by object/session contract; preserve subprocess, client and restart boundaries |
| Integrator | `tests/*.rs`, shared support registrations, inventory tooling and manifests | Check names/counts, register leaves, serialize Cargo validation and review |

The organization pass extracted tests from every Cargo root and moved existing leaves into their behavior directories. All 32 Cargo target names and CI shard assignments are unchanged. The compiled binaries list exactly the same 5,542 test instances as the current static registration graph.

## What this pass changed

- Organized all existing `tests/**` declarations and support files, with no orphan registrations, duplicate identities, unresolved paths or oversized Rust files.
- Reconciled all 138 remaining backfill items to registered tests or explicitly linked existing assertions. The ledger records corrected expectations and refined names.
- Reconciled all 28 historical oracle corrections and replaced or strengthened all 54 original parse-only checks. See [oracle corrections](oracle-corrections.md).
- Preserved 5,380 of the 5,387 baseline registered identities exactly. Seven deliberate replacements or deduplications are listed in [validation](validation.md).
- Added scalar cast, catalog, aggregate resource, text-search, range/index and lifecycle tests. No runtime, Cargo or CI changes were made.

Authored coverage, an observed failing test and complete feature qualification are separate states. The [validation record](validation.md) reports the full run and prerequisite failures. Named-client, complete corpus and immutable workload gates remain tracked in [the compatibility inventory](../postgresql18-test-inventory.md).

## Regenerate or check the inventory

The generator uses Rust syntax trees, Python ASTs, Cargo metadata and the backend shard manifest. It follows `mod`, `#[path]` and literal `include!` registrations. It includes `#[ignore]` declarations, does not evaluate `cfg`, and does not execute tests or infer their results. Macro-generated test cases and runtime-discovered counts need their owning runner's listing.

```sh
python3.12 -m venv target/test-inventory-tools312
target/test-inventory-tools312/bin/python -m pip install -r tools/test_inventory_requirements.txt
target/test-inventory-tools312/bin/python tools/test_inventory.py
target/test-inventory-tools312/bin/python tools/test_inventory.py --check
```

Edit `planned-tests.json` for assertion links or `layout-decisions.json` for reviewed locations, then regenerate. Generated pages contain no runtime pass/fail evidence. The generator removes stale generated owner pages and preserves this hand-maintained guide.
