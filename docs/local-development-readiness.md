# Local Development Readiness

This gate defines when Cassie is repeatable for real local schema, query, and
PostgreSQL-wire experiments. It is narrower than production qualification and
does not claim PostgreSQL server parity. The [Product Roadmap](product-roadmap.md)
owns release scope; [Feature Support](feature-support.md) remains the authority
for each SQL capability's current status.

## Boundaries

- Cassie uses `cntryl-midge` directly as its only storage layer.
- A transaction may change rows and catalog objects only inside one Midge
  database family. Cross-database transactions remain unsupported.
- PostgreSQL 18.6 is a semantic oracle for selected behavior, not a parity
  target.
- Correctness fixtures use Midge memory mode. Persistence, restart, and recovery
  cases use isolated local storage.
- GitHub Actions remain disabled. Local evidence is reported as local; hosted
  checks are unavailable.

## Latest selected v0.3 checkpoint — 2026-10-29

Main `d2993177204179fce3b54c7880744aca4a5f8017` contains the selected #777
client/catalog acceptance, the selected #846 COPY ingress and the #780 bounded
wire closeout. PR #1136's exact code/test candidate passed the ordered local
gate, including the full locked Cargo suite and all four pinned client
workflows. PR #1127 independently passed the same local build, test, strict
Clippy, formatting and focused-validator gate for selected COPY cancellation,
bounds and recovery. On current main, four focused COPY groups passed 136/136
tests across the library, compatibility, extended pgwire and core pgwire
targets. The merged client workflow
`should_preserve_named_tokio_postgres_session_through_error_recovery` passed
1/1 on current main. The full locked suite was not rerun on `d299317`. These are
local selected-workflow results, not hosted CI, desktop certification,
production qualification or a complete PostgreSQL wire state/packet matrix.

The unaliased WIRE-012 mixed catalog/ordinary prepared-join shape remains a
confirmed `42P18` gap, but the pinned workflow uses aliases and no named v0.3
client acceptance requires the unaliased form. It is recorded as deferred under
#780, which PR #1137 closed for its bounded selected scope. #846's broader
export/dump/restore forms are removed from v0.3; its selected rebuild ingress
is complete under PR #1127. The earlier branch-specific failure map below is
not the current main failure state; the selected wire/COPY items were later
requalified by #1127, #1136 and #1137.

## Historical checkpoint — 2026-10-29, pre-merge candidate

The current candidate branch is `audit/v03-826-cancel-refresh-20261029`, based on merged main `a4e5ec52566b4032c0a1e617e0b95d09c77107af` (PR #1121).
The owner moved #833's broader protocol, concurrency and catalog qualification
to v1.0 after
reconciling the selected v0.3 transactional-DDL runtime. Owner direction moved
named enums #827 to v0.4.

This candidate fixes cleanup when concurrent materialized-view refresh is
cancelled after replacement output and copied indexes are built but before
activation. The failed candidate is recorded, its physical output and copied
indexes are removed, and the prior output remains active. An injected
pre-publication error exercises the same cleanup path. Cancellation arriving
while activation metadata is being prepared is checked again before publication.
A local-storage test restarts after cancellation, confirms the failed candidate
and previous output recover correctly, then completes a subsequent refresh.
These five deterministic unit tests passed 5/5; the focused materialized-view
compatibility group passed 23/23 runnable cases with 2 explicit access-method
cases ignored. The final activation interruption check is the cooperative
cancellation boundary: a cancellation observed before it aborts and cleans the
candidate; one arriving after it may race with successful synchronous
publication, consistent with staged writes. A deterministic probe covers both
sides of that boundary. The preceding exact full gate passed 9,852/0/12 across
35 targets on main `44c04f680f4080ee02de334d4d63411c8f4d72f6` after PR #1120.
PR #1121 added one expression-index rollback witness; the exact-tree gate on
`a4e5ec52566b4032c0a1e617e0b95d09c77107af` passed: build; 9,853 passed, 0
failed and 12 ignored across 35 test targets; strict workspace/all-target/all-
feature pedantic Clippy; formatting; and touched-test validation (5/5).
Documentation validation covered 126 Markdown documents. The generated test
inventory reports 8,350 Rust declarations and 8,478 registered instances, with
no unregistered tests, unresolved modules, duplicate identities, shard errors
or oversized files. GitHub Actions remain disabled, so hosted checks are
unavailable and are not claimed as passed.

PR #1092 merged as `708c0e377b8ccc0ae7a4285b33964faf80100333`; its #839
regression `should_keep_unique_enforcement_after_using_type_rewrite_restart`
reproduces the numeric UNIQUE bypass on baseline and verifies typmod-aware
canonicalization, values/NULL, OID/typmod and named UNIQUE enforcement before
and after local restart. PR #1094 merged as
`848309a53c508573ff45a535519844405e2a76c5` and adds SQL materialized-view
relation identity through `pg_class`, `pg_attribute`, `regclass`, rename,
refresh and restart. The selected #842 partial-index restart regression
verifies committed entry changes remain queryable and match an independent
scan. PR #1103 added the #842 `lower(email)` restart witness and stabilized the pgwire cancellation test after a full-suite timeout. PR #1104 merged the #839 child-owned foreign-key rollback witness; the committed-drop restart path already had a separate witness. PR #1114 merged the #839 combined `NUMERIC(20,0) USING` conversion and literal-default column addition restart witness. PR #1115 added local-restart evidence for a composite secondary index built over populated rows and maintained through writes. PR #1095 added separate FK table-CASCADE tests for transaction rollback
and committed restart; the executor behavior was already present, so that was a
coverage and documentation correction, not a runtime change. Its touched-file
validator reports **15/15 compliant**.

PR #1097 moves explicitly owned, identity, SERIAL, and legacy-owned sequences
with their table across schemas. Its tests cover collision preflight, preserved
sequence identity and position, rewritten defaults, transaction rollback, and
local-storage restart. PR #1098 adds referenced-table schema-move witnesses:
rollback keeps the foreign key on the source relation, and a committed move
remains enforced after restart. The focused schema-move tests pass **9/9**; the
foreign-key lifecycle file validates **17/17**.

PR #1099 merged the #842 schema-move catalog fix as
`5a4d47cc714e9dc64bba8981ae843c53037c89ce`. It synchronizes catalog and durable
Midge index names while preserving index OID and unique-reservation identity;
#842 remains open for broader index-shape and lifecycle acceptance.

PR #1100 adds rollback and local-restart evidence for
`ALTER TABLE ... DROP COLUMN ... CASCADE`, including dependent-view and
foreign-key restoration/removal, referential integrity, and unrelated parent
data preservation.

The current #839 increment fixes the column-level `RESTRICT` foreign-key
diagnostic: PostgreSQL 18.6 returns `2BP01`, while the previous Cassie path
returned `22000`. The typed diagnostic now preserves the foreign-key name in
DETAIL and supplies the CASCADE hint. Focused tests cover inbound foreign-key
and view dependencies and verify the rejected operation leaves columns and
dependencies intact. A follow-up #839 regression reproduced a missing extended-
protocol NOTICE for absent `ALTER TABLE IF EXISTS`: PostgreSQL 18.6 emitted it
during Execute while Cassie emitted none. The focused test failed before the
fix and passes after the extended path writes the same NOTICE before the
command tag. Additional red/green cases verify that transaction-local CREATE
followed by ALTER emits no false notice over either Simple Query or extended
protocol; PostgreSQL 18.6 returned no notices for that transaction-local case.
#839 remains open for its other lifecycle forms.

The ordered local gate passes on this candidate branch: `cargo build --locked
--bin cassie`; `cargo test --locked` (**9,844 passed, 0 failed, 12 ignored
across 35 targets**); strict full pedantic Clippy; `cargo fmt --all -- --check`;
and touched-test validation (**8/8** for the scalar-index restart file).
`cntryl-tools validate-docs` validates **126 Markdown documents**. The checked
test inventory has 1,570 files, 8,346 Rust test declarations, 8,474
registered Rust instances, 21 Python test declarations, and 131 planned tests;
it reports no unregistered tests, unresolved module paths, repeated
registrations, duplicate identities, unsharded targets, or oversized files.
The generated inventory check passes on this branch.
Missing historical source paths remain linked to owner issues and are not
marked executed. These counts aggregate workspace targets and declarations
respectively.

Jev's focused adversarial question was whether the bounded #842 table
schema-move fix leaves a material correctness gap; the estimated probability
was **0.19**, below the **0.30** probe threshold. Source inspection and the
green rollback, catalog, index-plan, uniqueness, OID, restart, qualified-drop,
and scan tests supported PR #1099's completed slice. Jev did not review the
#839 CASCADE coverage, the RESTRICT correction, or the extended-query NOTICE
slice; its behavior is established by PostgreSQL 18.6 probes and focused
wire tests.

The nested `cpu_identity` stress harness prints `failed quality (no benchmark
rows)` inside a passing Cargo test target. This is not benchmark qualification.
The full suite initially hit a timeout in the pre-existing matching-backend
cancellation test; the test now retains its bound listener and synchronizes at
the `pg_sleep` entry boundary before sending CancelRequest. Both cancellation
tests and the subsequent full suite pass. Release readiness remains incomplete:
selected local-development/client workflows and exact-release-commit browser,
benchmark, and disk-backed operational evidence still need resolution. Actions
remain disabled, so hosted checks and publication are unavailable.

## Baseline discovery

The initial complete discovery run executed on 2026-10-21 from
`4572472db9e37c28037e0596d8628813be88d8e0`, including unpublished commit
`657a6413` and the preserved test-fixture patch
`b972dd24bc0c816b51965be32cc2420a3b9767fac85f257a0c03851170b6f629`. No
runtime source or test files changed during this run. Command:
`cargo test --locked --no-fail-fast`; exit status 101, **8,504 passed, 1,363
failed, 9 ignored**.

The failed identities are concentrated in these targets:

| Target | Failures | Representative identity and observed cause |
| --- | ---: | --- |
| `tests/pgwire_extended.rs` | 547 | `record_table_functions`, `postgresql18_money_compat`, `json_collection_sql_json_wave`, and `type_resolution_prepared_descriptors`; unsupported SQL/type forms and Describe/result metadata mismatches. |
| `tests/compatibility.rs` | 376 | `atomic_drop::catalog_visibility::should_publish_transactional_table_catalog_rows_after_commit` and `transactional_index_visibility::should_hide_transactional_index_after_rollback`; DDL is rejected inside an active transaction. `issue_839_atomic_lifecycle::should_persist_renamed_alter_relation_after_restart`; selected ALTER behavior is unsupported. |
| `tests/sql_queries.rs` | 266 | `postgresql18_aggregate_query_gaps` (57), `postgresql18_pattern_matching` (48), `postgresql18_text_search` (33), `postgresql18_relations` (16), `postgresql18_tablesample` (13), and `postgresql18_ctes` (12); shared parser, binder, planner, executor, and type gaps. |
| `tests/catalog_security.rs` | 123 | `type_catalog_discovery::identifier_residuals::should_preserve_regnamespace_oid_across_rollback_rename_drop`; DDL is rejected in a transaction. Most other failures are broader catalog/function coverage. |
| `tests/parser_types.rs` | 22 | `postgresql18_grammar` (14), `postgresql18_binary_bytea` (3), and `postgresql18_ctes` (3); grammar and type-specific rejection/descriptor mismatches. |
| `tests/sql_mutations.rs` | 13 | `postgresql18_transactional_ddl::should_restore_dropped_table_after_transaction_rollback` and `transaction_errors::should_publish_transactional_table_creation_only_after_commit`; DDL is rejected in a transaction. Repeatable-read snapshot cases are a separate, unselected isolation capability. |
| `tests/pgwire_core.rs` | 8 | COPY framing/options and assignment SQLSTATE mismatches; COPY is retained under its existing owner. |
| `tests/executor.rs` | 3 | `postgresql18_collection_aggregate_wave3`; aggregate semantic gaps. |
| `tests/metrics.rs` | 2 | Runtime metric behavior mismatches. |
| `tests/query_scan_controls.rs` | 1 | ANN concurrency fixture boundary mismatch. |
| `tests/search.rs` | 1 | Persisted full-text scoring memory-budget failure. |
| `tests/sql_exists_subqueries.rs` | 1 | Scalar cardinality is checked after the inner row limit. |

The high-frequency shared causes are 276 assertion mismatches where Cassie
returned SQLSTATE `42601` but the test expected no SQLSTATE, 118 `0A000
unsupported SQL statement`, nine explicit-
transaction DDL rejections, eight active-transaction DDL rejections, 28 result
descriptor mismatches, and 15 expected `42703` errors observed as parser
errors. These categories are not all in this gate: broad PostgreSQL catalog,
type, extension, isolation, and server-maintenance behavior retains its current
owner. Full baseline identities and observed causes were captured in the
ignored `target/local-development-readiness/baseline-failures.tsv`; the
grouped report is `target/local-development-readiness/baseline-groups.md`.

The issue-relevant first-batch clusters are explicit and reproducible:

- Transaction/catalog ownership: the transactional create, alter, drop,
  savepoint, and cross-session cases above fail with
  `DDL and catalog operations are not supported inside an active transaction`.
  They belong to the selected #833 contract. Broader catalog MVCC and
  repeatable-read requirements remain outside this local gate.
- Schema evolution: the selected #839 increments now pass
  `should_add_column_if_not_exists_without_replacing_existing_default`,
  `should_drop_schema_dependents_with_cascade`,
  `should_move_table_between_schemas_without_changing_its_rows`, and
  `should_persist_renamed_alter_relation_after_restart`. Focused green coverage
  includes name-only no-op behavior, literal-default backfill for historical
  and staged rows, pre-commit extended-protocol reads, rollback/commit, restart
  replay, partial-batch recovery, legacy journal decoding, and the selected
  `ALTER COLUMN TYPE ... USING` conversion with a sibling column addition.
  The conversion reports NUMERIC OID 1700 and typmod 1,310,724; division by zero
  returns 22012 without publishing sibling DDL, and a conversion that violates
  a unique constraint returns 23505 without changing the original rows/schema.
  This does not close #839; broader relation and object lifecycle acceptance
  remains with its owner. The ordinary serial-sequence table-rename case also
  passes through local restart and same-name table recreation; canonical
  catalog registration and sequence ownership are retained.
- Index lifecycle: `compatibility::lifecycle::ddl_extensions::should_drop_an_index_without_an_on_clause`
  and `...::index_contracts::drop_index_if_exists_scope::should_not_resolve_missing_schema_qualified_index_in_another_schema`
  failed because the parser required `ON`. This defect is addressed by the
  current #842 increment. Unnamed `CREATE INDEX ON table (...)` and truthful
  `pg_indexes.indexdef` output are also covered. Selected parser, planner,
  pgwire, catalog, and numeric indexed-versus-scan regressions now pass. The
  PostgreSQL-specific `DROP INDEX CONCURRENTLY` case remains outside this
  gate's standard index forms and is still tracked by #842. The no-`ON` parser
  change first treated `CONCURRENTLY name` as an index name; a focused
  red/green regression now rejects it explicitly as unsupported syntax. The
  PostgreSQL-client success case still fails outside this gate:
  `compatibility::lifecycle::ddl_extensions::index_contracts::should_drop_index_concurrently_outside_a_transaction_block`
  returned `42704` for the misparsed name before the guard and now returns
  SQLSTATE `0A000` with an explicit unsupported-feature message. Partial-index
  key modifiers and other index forms remain pending.
- Relational queries and pgwire: aggregate, pattern, text-search, CTE,
  subquery, type-codec, and descriptor failures span shared components. They
  need the generic relational corpus and named-client gates before their owner
  disposition can be called complete.

The focused post-baseline storage-index run also reproduced two tests outside
this gate's selected type carriers: `types_ranges_operators::should_match_scan_results_given_indexed_range_values_when_range_predicates_execute`
and `types_ranges_operators::should_match_scan_results_given_a_custom_range_with_subtype_diff_when_its_index_is_queried`.
They fail because `INT4RANGE` and custom `CREATE TYPE ... AS RANGE` are not
implemented; both remain under existing [#905](https://github.com/cntryl/cassie/issues/905).
No issue was opened for this classification.

The storage-index target passed **188 tests, 0 failed, 1 ignored** in the same
baseline, confirming the existing #953 reservation-backing repair. It is not
reimplemented here. The baseline inventory is not a claim that all 1,363
failures have been individually mapped out of this gate; that triage remains an
exit requirement.

## Post-baseline verification

After the index name-resolution and unnamed-index changes, `cargo build --locked --bin
cassie` passed. The focused parser index module passed **31/31**; generated-name
compatibility passed **1/1**; DROP INDEX scope passed **2/2**; unqualified and
schema-qualified DROP INDEX passed **1/1 each**; the hash access-method catalog
case passed **1/1**; planner command tests passed **9/9**; pg_indexes catalog
definition passed **1/1**; and numeric indexed-versus-scan passed **1/1**. A
broader substring filter `cargo test --locked --test parser_types index` also
selected `parser_types::postgresql18_grammar::should_index_a_postgres_array_after_casting_it`:
33 tests passed and that one test failed with `UndefinedColumn` after a cast.
The same test identity and cause are present in the complete baseline inventory
above, so it is an existing array-parser gap rather than a regression in the
index changes. `cargo fmt --all -- --check`, `git diff --check`, and test
validation for the touched test files passed. A red/green parser regression
also verifies that the explicitly unsupported `DROP INDEX CONCURRENTLY` form
cannot be mistaken for an ordinary index name. The storage-index suite completed
with **188 passed, 2 failed, 1 ignored**; its only failures are the two #905
range-type cases above. The constraint namespace backing and restart cases
passed. The database-image fixture was restored to isolated local storage
because aborting an incomplete restore exercises durable family cleanup; the
row-schema test now reads the default database family where the current catalog
layout stores that metadata.

After correcting SERIAL sequence registration and table-rename ownership,
`cargo test --locked --test sql_mutations
migration_ddl_sequences::should_rename_serial_sequence_when_renaming_its_table
-- --nocapture` passed **1/1**, and the complete focused
`migration_ddl_sequences::` module passed **8/8**. That regression uses isolated
local storage, restarts Cassie after the rename, verifies the old sequence name
is absent, then inserts through the renamed table and recreates the old table
name. `cntryl-tools validate-tests -f
tests/sql_mutations/ddl/migration_ddl_sequences.rs` passed **8/8** and the
workspace formatting check passed. This focused evidence does not replace the
full batch validation sequence.

The #762 scalar-subquery cardinality case was first rerun red: with
`max_result_rows = 1`, a two-row scalar subquery returned `ResourceLimit`
before the scalar evaluator could report its PostgreSQL cardinality error.
Scalar and row subqueries now evaluate their bounded inner plan with the
existing uncapped-input controls, then apply the configured row cap to the
subquery result after cardinality validation. `cargo build --locked --bin
cassie` passed; `cargo test --locked --test sql_exists_subqueries` passed
**61/61**; `cargo fmt --all -- --check` passed; and
`cntryl-tools validate-tests -f tests/sql_exists_subqueries/scalar_in_any_all.rs`
passed **35/35**. The full workspace/all-target/all-feature pedantic Clippy
command also passed after two existing index-slice lint findings were fixed.
The full locked test suite remains required at the batch boundary.

The complete locked suite was rerun on 2026-10-22 after the hash-method
rejection and graph assertion changes, before the later Clippy-driven extraction
of the hash guard into a helper: `cargo test --locked --no-fail-fast` exited 101
with **8,517 passed, 1,529 failed, and 9 ignored**. Ten integration targets
still fail:
`catalog_security` (122), `compatibility` (372), `executor` (3),
`parser_types` (22), `pgwire_core` (8), `pgwire_extended` (637),
`sql_mutations` (18), `sql_queries` (344), `storage_indexes` (2), and
`vector_embeddings` (1). The exact current inventory is captured in the ignored
`target/local-development-readiness/final-full-test.log` and
`current-failures.tsv`; this remains a failed full-suite result.

Comparing current failure identities with the baseline inventory, **14 baseline
failures now pass and 180 current failures were absent from the baseline failure
set**. The 180 current-only failures are mapped as follows:

- 166 geometry, range/multirange, and XML cases remain under existing #907,
  #905, and #906 respectively. Those type families are outside the selected
  local-development carriers and remain future capability work. The two
  indexed range cases are the existing #905 gap.
- Five foreign-key rename/drop lifecycle cases remain under #839.
- Seven pgwire/session cases remain in the selected #780/#833 workflow: declared
  result metadata, pagination, implicit transaction status, failed-transaction
  recovery, portal capture, portal cleanup, and portal resume after savepoint
  rollback.
- One unique-key upsert waiter case fails under #819 when the owner rolls back.
  The owner-commit variant also remains a baseline failure. This does not
  replace or reimplement the existing #953 reservation-backing repair.
- One exact-vector query memory-budget case remains with the separate vector
  qualification work; vector expansion is not part of this local gate.

All 1,349 baseline failures that remain in the current run retain their prior
grouped dispositions above; this comparison is an identity delta, not a claim
that the selected gate has passed. Fourteen former failures now pass, including
the selected generated-index-name, no-`ON` DROP INDEX, index-definition,
numeric indexed-read, and scalar-subquery cardinality regressions. The graph
manifest unit also passes after its assertion was aligned with canonical
database/schema-qualified catalog names. A new compatibility regression proves
that scalar `USING hash` is rejected with SQLSTATE `0A000` until hash storage is
implemented; reporting it as a hash index while using the scalar path would be
misleading.

After that full run, `cargo build --locked --bin cassie` passed on the final
helper structure; the focused hash-rejection regression passed **1/1**;
`cargo clippy --workspace --all-targets --all-features -- -D warnings -D
clippy::pedantic`, `cargo fmt --all -- --check`, and `git diff --check` passed.
`cntryl-tools validate-tests -f <path>` passed for all **400 touched Rust test
files**, covering **1,887/1,887** recognized test cases; 51 touched support or
module files contained no directly validated test cases.

The focused upsert, schema-evolution, and pgwire failures remain open behavior;
they must be resolved or explicitly dispositioned before this gate can be
accepted. Do not report the full suite as green or the 10 failing targets as
closed.

Initial delivery forecast: **10–16 full-time engineering weeks from this
baseline**, assuming one primary engineer, stable capacity, and no new storage
format or public Rust API decision. Confidence is low until Batch 1 demonstrates
catalog and row staging together. Reforecast at that batch boundary. The raw
baseline contains substantial future-capability failures, so the forecast
changes if individual disposition shows they are inside the selected gate.

## Implementation Batches

### 1. Schemas, transactions, constraints, and scalar indexes

Stage database-scoped catalog changes with row changes. Support transactional
table and scalar-index creation, alteration, and removal, including savepoint
rollback. Make the selected schema lifecycle dependable: qualified schemas and
tables, defaults and identity values, named primary/unique/foreign-key/supported
CHECK constraints, supported column evolution, and scalar single/composite,
unique, partial, expression, and covering indexes. Preserve atomic multi-row
writes, upserts, `RETURNING`, named errors, and cache invalidation. Verify the
existing #953 reservation-backing repair rather than reimplementing it.

Acceptance requires owner-session visibility, isolation from other sessions
until commit, rollback after failed DDL and writes, constraint enforcement,
index-versus-scan equivalence, and retained state after restart.

### 2. Relational query combinations

Use two reusable generic relational fixtures with multiple schemas, related
tables, nullable fields, composite keys, and varied types. Exercise joins,
grouped aggregates and `HAVING`, CTEs and bounded recursion, correlated and
`EXISTS`/scalar/`IN` subqueries, set operations, windows, ordering, pagination,
and combinations through views. Check current scalar, UUID, bytea, temporal,
JSON, and array carriers through storage, expressions, parameters, and results.

Compare supported values, multiplicity, explicit ordering, NULL behavior,
errors, and result metadata with pinned PostgreSQL 18.6. Compare indexed plans
to an independent scan before and after writes, rollback, schema changes, and
restart. Use `EXPLAIN` and counters to prove selected index use. Run the corpus
through pgwire simple, extended-text, and extended-binary modes.

### 3. pgwire and the local development loop

Verify parameter inference, NULL and text/binary codecs, truthful Describe
metadata, prepared statement reuse and schema invalidation, Query/Sync boundaries,
commit failures, savepoints, aborted transactions, portal paging, cancellation,
deadlines, disconnect cleanup, session settings, and recovery after errors.
Exercise selected psql/libpq workflows plus the existing tokio-postgres, sqlx,
and Diesel probes for schema creation, parameterized writes, relational reads,
transactions, discovery, and error recovery.

Document one native launch path and one locally built Compose path with persistent
storage, explicit credentials, selectable ports, and actionable startup errors.
Provide a repeatable create/load/query/migrate/restart/reconnect exercise and
reset instructions for disposable data. Run existing query and pgwire benchmark
smokes once at the final checkpoint.

## Exit Criteria

- Every selected schema, scalar-index, relational-query, and pgwire acceptance
  case passes.
- A complete test run has no untriaged failure or unexplained regression.
  Remaining future-capability failures are individually reproduced and mapped
  to existing owners; the full-suite result remains failed while any such test
  still fails.
- There are no known wrong results, lost acknowledged writes, broken rollback,
  stale index answers, protocol desynchronization, or crashes in this gate.
- Representative queries work within documented resource settings, and
  cancellation and limit errors leave the server usable.
- The final evidence names the exact commit, configuration, commands, results,
  and remaining limitations.

At each batch boundary, run the ordered local sequence: build, locked full tests,
strict workspace/all-target/all-feature pedantic Clippy, formatting check, and
touched-test policy validation. Review substantial diffs with Jev and source
inspection; turn confirmed findings into regression tests. Deliver one
substantial PR at a time. Do not change or dispatch CI/release workflows under
the current Actions-disabled direction.

## Starting Evidence

Baseline discovery is complete with results and representative failed test
identities recorded above. The full failure inventory is reproducible with the
same command and preserved baseline patch. No batch is accepted until its exit
criteria and ordered validation complete.
