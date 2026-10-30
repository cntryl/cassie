# Backfill and organization validation

The 2026-10-27 [relational qualification checkpoint](relational-qualification-2026-10-27.md)
records 22 executed primary witnesses, the local full-run failures, and the
remaining #761 acceptance and dependency gates. It does not qualify the full
relational contract or close #761.

> **Historical validation archive.** The runs and issue counts below describe
> their recorded revisions and dates. They do not report the current main head,
> current GitHub issue state, or an active full-parity product goal. PostgreSQL
> 18.6 observations are reference evidence for selected behaviors only.

## 2026-10-28 — #833 standalone DDL preflight

A focused pgwire probe sent `CREATE TABLE` followed by `INSERT` in one
`batch_execute` Simple Query. Cassie returned SQLSTATE `0A000` with
`DDL and catalog operations require a standalone Query`, before creating the
table. This matches the selected [wire transaction contract](../pgwire-transaction-contract.md),
which rejects mixed Simple Query messages containing DDL before execution. The
regression `should_reject_mixed_simple_query_ddl_before_execution` asserts the
table is absent and the same connection accepts a follow-up query; it passed
1/1. The initial positive mixed-query expectation was outside the documented
profile, so no runtime behavior changed. This is one negative boundary case and
does not complete #833 or qualify a release candidate.

## 2026-10-28 — #839 numeric type rewrite uniqueness

On main `364b801f`, a focused local-storage probe created an `INT UNIQUE`
column with value `22`, rewrote it to `NUMERIC(20,2) USING amount::numeric`,
and attempted to insert `22` again. The duplicate succeeded both before and
after restart. The rewritten value uses NUMERIC's text storage form while an
integer literal reached validation as a JSON number; exact JSON comparison
therefore missed a SQL-equal value. The regression
`should_keep_unique_enforcement_after_using_type_rewrite_restart` first failed
on that baseline.

NUMERIC values now pass through the declared numeric typmod before UNIQUE
validation and storage. The focused regression passes and checks NULL/value
preservation, OID 1700 and typmod across local restart, and named UNIQUE
rejection for integer and decimal duplicates both before and after restart.
This adds no public API or persistent layout. The ordered local gate passed on
the final branch tree: `cargo build --locked --bin cassie`; `cargo test --locked`
(9,813 passed, 0 failed, 12 ignored across 35 targets); strict full pedantic
Clippy; `cargo fmt --all -- --check`; and
`cntryl-tools validate-tests -f tests/compatibility/lifecycle/ddl/atomic_lifecycle.rs`
(11/11 compliant). This qualifies the local #839 increment only; no
release-readiness claim is made.

## 2026-10-28 — #842 partial scalar-index write maintenance

Added `should_maintain_partial_index_entries_through_predicate_changing_writes`
to verify the selected scalar-index contract over pgwire. It exercises a
partial covering index through rollback of a staged update and insert, a
committed matching insert, updates that add/remove rows from its predicate and
change indexed/INCLUDE values, and deletion,
`EXPLAIN` confirmation that the exact predicate uses the index, and result
comparisons against independent scans before writes, after rollback, and after
committed changes. The focused test passed
1/1 on the current worktree. An initial probe used a query that strengthened the
partial predicate; Cassie correctly declined that unsafe index alternative, so
the final probe uses the exact indexed predicate. No runtime behavior changed.
This evidence covers one write-maintenance shape; it does not complete #842 or
qualify a release candidate.

## 2026-10-29 — #842 partial-index entry recovery after restart

Added `should_recover_partial_index_entries_after_committed_writes_on_restart`
to the disk-backed scalar-index restart suite. It commits an insert, updates
that add/remove rows from the predicate and change an INCLUDE value, and a
delete. After local restart, the eligible query still selects the partial
covering index; results equal an independent scan after dropping that index.
The focused test passed 1/1 and the touched-file policy validator passed 6/6.
The ordered full local gate also passed on this tree:

- `cargo build --locked --bin cassie`
- `cargo test --locked` — 9,814 passed, 0 failed, 12 ignored across 35 targets
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f tests/storage_indexes/indexes/scalar_indexes/restart.rs` — 6/6 compliant

No runtime behavior changed. This is one persisted partial-index write shape;
the complete #842 index lifecycle remains open and the release gate is not
qualified by this focused result.

## 2026-10-29 — #826 SQL materialized-view relation identity

Source checkout base: `71749182f3b755ac33d9423d1381328efc5d1fe8`; work was
performed on an isolated `issue-826-matview-catalog-identity` branch. The new
pgwire compatibility test first failed on that base because `pg_class` returned
no SQL materialized-view row. A focused legacy-metadata regression also
reproduced an OID change on rename when `projection_id` was empty. Rename now
seeds the existing identity field from the current collection name before
changing the name; no metadata field or storage format was added.

The selected behavior exposes `relkind = 'm'` and heap access method OID 2 in
`pg_class`, output columns in `pg_attribute`, and `regclass` conversion in both
directions. Tests cover an unpopulated view, namespace and column identity,
refresh, same-schema rename, drop cleanup, and local restart. Relation identity
remains stable across refresh, rename, and restart. Jev's first semantic review
raised a possible legacy-identity gap (0.54); the targeted empty-ID probe
confirmed it and drove the rename repair. The final adversarial review selected
no remaining gap (0.99); this judgment is review input, while the regression
and source behavior provide the evidence.

The ordered local gate passed on the final code/test tree:

- `cargo build --locked --bin cassie`
- `cargo test --locked` — 9,816 passed, 0 failed, 12 ignored across 35 targets
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
- `cargo fmt --all -- --check`
- Touched-test validation: compatibility identity 1/1, materialized rename
  lifecycle 12/12, and restart publication 2/2 compliant

`cntryl-tools validate-docs` still reports existing broken links elsewhere in
the documentation/testing inventory and reports no errors in the files changed
by this batch. GitHub Actions remain disabled; hosted checks are unavailable.
This delivers one #826 catalog subcase only. The remaining #826 obligations,
v0.3 scope freeze, selected client workflows, and exact-release-commit browser,
benchmark, and disk-backed operational evidence remain open; this is not a
release-readiness claim.

This section records the test-only backfill and organization pass on 2026-10-15 on `feature/postgres-18-compat`, from checkpoint `00000118` (which preserved the existing dirty backfill). One branch and one worktree were used. That pass did not change runtime source, Cargo configuration, corpus expectations or CI configuration, and claimed no runtime fixes or issue closures.

The user subsequently authorized runtime implementation on the same branch. The historical counts and commands below describe the backfill checkpoint, not the later working tree. Current implementation checks must name their source revision and scope; focused passes do not establish the full compatibility gate.

## Inventory and identity

- All 138 identified remaining-list items link to 136 distinct registered tests (two pairs share bodies that assert both linked contracts): 124 have the inventory name, 13 link to refined names or existing equivalents, and one invalid large-object rejection proposal was corrected to the PostgreSQL-positive behavior.
- All 69 open milestone owners have exact finite assertion anchors in [owner-assertions.md](owner-assertions.md). Closed milestone and historical owners remain scoped regression evidence in [the compatibility inventory](../postgresql18-test-inventory.md).
- The baseline contains 5,263 Rust declarations and 5,387 registered instances. The current tree contains 5,414 Rust declarations and 16 Python declarations. Current registration totals are generated in [the summary](summary.md).
- Cargo target names and CI shard membership remain unchanged: 32 targets, each in exactly one shard. No orphan tests, unresolved paths, repeated registrations, duplicate test identities or oversized Rust files remain. All 1,535 files are indexed, including 1,504 Rust files.
- 5,380 baseline registered identities are preserved exactly. The seven exceptions below correct expectations or remove a duplicate. Moving a file did not otherwise rename its tests.

| Baseline test suffix | Current assertion / reason |
| --- | --- |
| `sql_string_literals::should_parse_like_escape_as_a_pattern_expression` | `should_apply_escape_characters_given_literal_wildcards_when_matching_like_patterns`: distinguishing executed true/false values |
| `sql_string_literals::should_reject_concatenation_instead_of_parsing_one_string_literal` | `should_evaluate_concatenation_given_literals_and_columns_when_used_in_queries_and_dml`: executed SELECT and DML values |
| `pgwire_boolean_binary::should_normalize_boolean_elements_without_changing_legacy_array_identity` | `should_reject_legacy_boolean_array_oid_before_result_metadata`: approved standard-array identity policy |
| `pgwire_boolean_binary::should_reject_trailing_boolean_array_container_bytes_as_malformed_input` | Removed duplicate legacy-OID case; standard-OID malformed framing is asserted by `should_classify_trailing_boolean_array_bytes_as_malformed_binary_data` |
| `pgwire_scalar_receive_errors::should_classify_nested_scalar_receive_errors_with_legacy_array_metadata` | `should_classify_nested_scalar_receive_errors_with_standard_array_metadata`: standard PostgreSQL OIDs |
| `sql_semantic_regressions::should_reject_non_finite_float_literals` | `should_preserve_numeric_literals_given_exponents_beyond_float8_range_when_selected`: `1e400` is a finite exact NUMERIC |
| `sql_semantic_regressions::should_reject_out_of_range_integer_literals` | `should_infer_numeric_given_integer_literals_above_bigint_range_when_selected`: exact NUMERIC value and OID 1700 |

Shared fixture registration was consolidated after Clippy found the same fixture loaded through three new storage leaves. Eight redundant helper instances introduced during authoring were removed; no baseline identity was lost by that consolidation. The nine existing ignored declarations remain ignored.

## Final authoring confirmation

The final readback inspected source declarations and module registrations without executing tests. All 138 planned items resolve to their current source functions and exact registered identities, representing 136 distinct tests. The inventory was regenerated after the last 15 naming refinements. No tests were run after the user directed this phase to remain authoring and organization only.

The first #762 scalar-cardinality case was then narrowed: its earlier one-row scalar-subquery positive control failed with 42601 before the multirow cardinality assertion. The current test retains a normal table/data control and checks the multirow expression's 21000 SQLSTATE and message directly. This refinement has not been executed.

The following #762 scalar-width case had the same early-control problem. Its one-column scalar positive control was removed; the current test preserves the ordinary table control and checks the two-column scalar query's 42601 plus exact width diagnostic. This refinement has not been executed.

The four #762 row-valued `ANY`/`ALL` cases shared a row-comparison positive control that failed with generic 42601. It is now a plain two-column table control, leaving each quantified expression as the operation under assertion. These refinements have not been executed.

The #762 uncorrelated and correlated evaluation-count cases now create sequences with PostgreSQL's default start value instead of the unsupported `START WITH` option. Their earlier setup failure was 0A000; these refinements have not been executed.

The #762 cancellation baseline and cancellable query now call PostgreSQL's `pg_sleep(float8)` form; the earlier `INTERVAL` input failed parsing before the cancellation protocol was exercised. The current query still checks active-backend observation, 57014, no partial rows, and same-session recovery. This refinement has not been executed.

The #762 result-limit case now keeps the ordinary two-row control and applies its 54000, no-partial-result, and recovery assertions directly to the three-row scalar query. Its former within-cap scalar positive control failed before the limit assertion. This refinement has not been executed.

For the two #799 multi-character escape cases, the invalid-input SQLSTATE/message assertion now runs before the valid LIKE/SIMILAR TO control assertion. Earlier runs stopped at those controls (0A000 for LIKE, 42601 for SIMILAR TO), so the invalid-input responses were not reported. These assertion-order refinements have not been executed.

Both #806 ordered-set resource cases no longer depend on `generate_series` support: they preload ordinary table rows before starting the instance with the resource limit. The cancellation case uses `pg_sleep(float8)` and waits for its query marker in `pg_stat_activity` before sending CancelRequest. These refinements have not been executed.

The #808 nondefault-bound array test now reports the `generate_subscripts` query before its separate bounds-control result. The earlier run stopped at the bounds control and did not report the target query. This assertion-order refinement has not been executed.

The #808 correlated LATERAL ordinality case now reports its `LIMIT 4` target query before the unlimited control. Its earlier run stopped while parsing the control's table-valued function. This assertion-order refinement has not been executed.

The #809 JSONPath cancellation query now uses an uncast path string, matching the sibling JSON expansion call. Its earlier positive control reported 42601 with an explicit `::JSONPATH` cast; the cause is unconfirmed. The earlier result is preserved in the ledger, and this refinement has not been executed.

The first #810 timezone-aware JSONPath result case now relies on unknown-string inference for the function's `jsonpath` argument, retaining the same predicate and expected JSONB result. Its earlier 42601 observation is preserved; the cause is unconfirmed and this refinement has not been executed.

The second #810 no-match case now uses the same inferred `jsonpath` argument form, retaining its later-offset predicate and SQL NULL expectation. Its earlier 42601 observation is preserved; the cause is unconfirmed and this refinement has not been executed.

The #811 string-aggregate budget case now seeds 50,000 rows into an ordinary table before opening the 4 KiB-budget instance. Its previous three-row `VALUES` control returned 0A000 before reaching the aggregate; the old observation is retained, and the table-backed control, row-count check, and 54000 candidate have not been run.

The #811 string-aggregate cancellation case now uses the same 50,000-row table fixture for its small control and marked target query. Its prior `VALUES` control returned 0A000 before CancelRequest; that observation is preserved, and this refinement has not been run.

The #811 array-aggregate budget case now seeds a 50,000-row table and uses it for the 3-row cardinality control, nonretaining row-count control, and 54000 candidate. Its earlier `VALUES` control returned 0A000; that result is preserved and this refinement has not been run.

The #811 array-aggregate cancellation case now uses the same 50,000-row table fixture for its control and cancellation candidate. Its prior `VALUES` control returned 0A000 before CancelRequest; the observation is preserved and this refinement has not been run.

The #811 JSON-aggregate budget case now seeds 50,000 rows into a table and uses it for its three-row JSON control, nonretaining count control, and 54000 candidate. Its earlier `VALUES` control returned 0A000; that observation is preserved and this refinement has not been run.

The #811 JSON-aggregate cancellation case now uses the same 50,000-row table fixture for its control and cancellation candidate. Its prior `VALUES` control returned 0A000 before CancelRequest; the observation is preserved and this refinement has not been run.

The #811 JSONB-aggregate budget case now seeds 50,000 rows into a table and uses it for its three-row control, nonretaining count control, and 54000 candidate. Its earlier `VALUES` control returned 0A000; that observation is preserved and this refinement has not been run.

The #811 JSONB-aggregate cancellation case now uses the same 50,000-row table fixture for its control and cancellation candidate. Its prior `VALUES` control returned 0A000 before CancelRequest; the observation is preserved and this refinement has not been run.

The #811 JSON object aggregate budget case now uses 50,000 unique table keys instead of `VALUES` sources and a `||`-built key expression. The earlier control returned 0A000; that observation is retained, and the table-backed control and 54000 target have not been run.

The #811 JSON object aggregate cancellation case now uses the same unique-key table for its control and active cancellation target. The earlier `VALUES` control returned 0A000 before CancelRequest; that observation is preserved and this refinement has not been run.

The #811 JSONB object aggregate budget case now seeds 50,000 unique integer keys into a table and uses it for its control, nonretaining row-count control, and 54000 candidate. Its previous `VALUES` control returned 0A000; that observation is preserved and this refinement has not been run.

The #811 JSONB object aggregate cancellation case now uses the same unique-key table for its control and active cancellation target. The previous `VALUES` control returned 0A000 before CancelRequest; that observation is preserved and this refinement has not been run.

## Previously recorded execution

The complete `cargo test --locked --no-fail-fast` run executed all 32 integration targets, library tests, binary tests and doc tests. It finished with **3,787 passed, 2,683 failed and 9 ignored**. All 930 library unit tests passed. Of the 138 reconciled backfill items, **136 failed and 2 passed**. The two passes assert binary cast-chain identity and UUID binary persistence; they are finite positive behavior evidence.

That broad run preceded the final cancellation/large-payload/I/O/extension-gating refinements and shared-fixture deduplication. Changed assertions were subsequently compiled and rerun separately. The final 15 name-only refinements were compiled but were not executed again; their current identities were checked statically. Do not interpret the broad totals as a current-head full-pass claim. Per-item observations and the broad suite totals are in [execution-results.json](execution-results.json); [the backfill ledger](planned-tests.md) keeps authoring and execution status separate.

| Command or probe | Result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed |
| `cargo test --locked --no-run --message-format=json` | Passed: all 32 integration targets compiled on the final test source |
| Compiled binaries with `--list --format terse` | Passed before the final 15 name-only refinements: 5,542 names matched the static registration graph; current names were checked statically afterward |
| `cargo test --locked --no-fail-fast` | Completed, exit 101: 3,787 passed / 2,683 failed / 9 ignored; intentional compatibility failures remain |
| `cargo test --locked --test catalog_security catalogs_ -- --nocapture` | 30 failed / 316 filtered; updated catalog, policy and large-payload assertions remain RED |
| `cargo test --locked --test pgwire_extended cancel -- --nocapture` | 2 passed / 27 failed / 1,108 filtered; same counts after the final helper refactors |
| `cargo test --locked --test compatibility row_lock_cancellation -- --nocapture` | 2 failed / 699 filtered; owner-lock/progress controls retained |
| `cargo test --locked --test compatibility pgcrypto_extension:: -- --nocapture` | 7 failed / 694 filtered; same counts after the final before-install guard refresh |
| `cargo test --locked --test pgwire_extended cast_chain_identity` | Passed: 1 binary cast-chain test |
| `cargo test --locked --test pgwire_extended uuid_binary_identity` | Passed: 1 UUID persistence test |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed before the final 15 name-only refinements: zero warnings or errors |
| `cargo fmt --all -- --check` | Blocked by existing formatting in unmodified `src/sql/mod.rs:37` and `src/sql/parameters.rs:287`; no source edits made |
| Scoped `rustfmt --edition 2021 --check --config skip_children=true` | Passed on all 1,504 Rust files before the final 15 name-only refinements |
| `cntryl-tools validate-tests -f <current Rust file>` | Passed on all 1,504 Rust files before the final 15 name-only refinements |
| `target/test-inventory-tools312/bin/python tools/test_inventory.py --check` | Passed: generated output matches the current source |
| `cargo test --locked --test pgwire_extended mutations_ -- --nocapture` | 4 failed / 1,133 filtered; final mutation progress witnesses retained |
| `cargo test --locked --test backend_ci` | Passed: 2 tests; exactly one shard for each of 32 targets |
| `git diff --check` | Passed on the final patch |

## Oracle and review evidence

The PostgreSQL 18.6 server and executable hashes match the pinned reference manifest. All 28 historical correction IDs are reconciled (27 SQL observations plus one protocol observation); the original 54 parse-only checks now map to 47 executed-value/metadata assertions, four structural AST assertions, one AST plus execution assertion, and two error assertions. See [oracle-corrections.md](oracle-corrections.md). These labels describe test contents, not Cassie passes.

Additional focused probes confirmed statistics restoration values, 17 backend-statistics/signaling observations, extension-gated `digest()`, and three live mutation cancellation scenarios. The latter observed sequence progress and the exact target backend waiting in `PgSleep`, then SQLSTATE 57014, no DataRows, rollback and reusable connections. The rule probe caught a real test-design error: source RETURNING runs before DO ALSO. The final test uses ordered rule actions, with its mutation before the sleep.

Jev's post-mutation evidence-gap probability was 0.77. The focused live PostgreSQL probe confirmed the gap in the original rule ordering; the tests were corrected and all three final oracle probes passed. Its supported completion statement was limited to authoring the 138 identified remaining-list items, with external qualification gates separate. Source, assertions and runtime observations remain authoritative.

## Limits on the observed failures

The workspace formatter remains blocked only by the two pre-existing source differences listed above. All test files passed formatting before the final name-only refinements; the six renamed files were formatted afterward. The task was kept test-only.

A positive control must succeed before an intended rejection or cancellation can count as target-path evidence. The following tests are correctly authored but currently stop at earlier requirements:

- Scalar subqueries, VALUES and recursive CYCLE fail their parser/execution controls before the intended cardinality, width, resource or cancellation assertions.
- The #812 UPDATE RETURNING test's 60-digit NUMERIC seed is valid PostgreSQL input and directly exercises the requested beyond-38-digit numeric support. Its prior INSERT failure is the intended red for that prerequisite; the test remains unchanged and has not been rerun.
- The #812 numeric-domain restart test depends on `CREATE DOMAIN` before it can persist and reload the NUMERIC(50,20) value. Its prior unsupported-statement result is a required domain-lifecycle prerequisite for this contract; the test remains unchanged and has not been rerun.
- The #814 indexed-instant and named-zone restart tests retain `TIMESTAMPTZ` columns as their target behavior. Their prior unsupported-type results are the expected red for that missing temporal type path; both tests remain unchanged and have not been rerun.
- Math domain tests fail supported-function controls before their final SQLSTATE assertions.
- The #813 overload test now inspects each rejected signature before its unary control, preserving the prior 42601 ROUND-control observation. The new assertion order has not been executed.
- The #813 DIV test now checks the zero-divisor response before its nonzero control, preserving the prior 42601 unsupported-DIV observation. The new assertion order has not been executed.
- Statistics restore/clear tests stop while binding valid typed arguments. Backend statistics stop at missing statistics functions. Current `pg_terminate_backend` execution reports unsupported function 42601; an earlier socket timeout is superseded and is not proof of successful signaling.
- Role-sensitive catalog/rule/large-object tests stop at the #831 SET ROLE dependency. The approved physical-helper 0A000 and commit-tracking-off 55000 tests include signature/NULL/privilege controls; unsupported unrelated syntax cannot satisfy those assertions.
- Mutation cancellation tests require sequence progress and `PgSleep` after actual mutation/action work. Other resource cancellation tests prove a marked active query plus no partial output and recovery; they do not claim a specific amount of retained state was already allocated.
- Parallel aggregate tests observe declared support metadata, partial combine selection, worker metrics and exact output. They do not independently prove serialization callback invocation.

Pinned pgvector is absent from the local PostgreSQL installation. Prisma, complete owner/form/path corpus execution, immutable workload/resource/restart profiles and native benchmark artifact identity remain qualification gates under #847–#849 and #927. Pending, ignored or reference-only cells are not Cassie passes.

## Implementation batch resumed on 2026-10-15

The uncommitted `feature/postgres-18-compat` implementation batch was built once,
then the seven affected test targets were compiled together. Library, parser/type
and executor targets ran in full; catalog, session/transaction, SQL aggregate and
extended-wire targets used retained explicit test-name selections. These are local
working-source observations, not an exact-commit full-suite or release gate.

| Target | Passed | Failed | Selection |
| --- | ---: | ---: | --- |
| Library | 963 | 0 | Complete target before the final FILTER correction |
| Parser/types | 288 | 83 | Complete target |
| Executor | 108 | 3 | Complete target |
| Catalog/security | 3 | 0 | Underlying type identity and canonical encoding |
| Compatibility | 25 | 111 | Session/transaction subtree |
| SQL queries | 7 | 82 | Boolean aggregates and aggregate query/semantic gaps |
| Extended wire | 24 | 8 | Casts, dollar quotes, lexical literals, aggregate errors and portal completion |

Comparison with the retained frozen `0e8dc34d` baseline found 46 previously
failing cases passing and no previously passing observed case failing. The wire
baseline is incomplete, so its newly observed passes are not counted as fixes.
The new CASE/function witness still fails the int4 arithmetic descriptor assertion;
its value assertions succeed. Collection/statistical aggregates, missing query
forms, locking and wider isolation remain failing obligations.

The wire modifier matrix exposed a further admission defect: an integer-typed
NULL FILTER passed Parse instead of returning `42804`. Adding FILTER to the
existing Boolean diagnostic selection fixes that phase without changing parameter
inference. The 13 parameter-occurrence tests pass, including typed NULL and known
non-Boolean parameter rejections, Boolean NULL controls and unknown-parameter
Boolean inference. The previously failing real-table simple-query/Parse-Flush
modifier matrix also passes. Jev assigned this correction a gap probability of
0.31; those focused negative and positive probes were executed and passed.
The subsequent complete library run passed all 965 tests on the corrected source
(`target/green-filter-lib.log`); the earlier seven-target totals above retain
their original snapshot boundary.

Evidence commands and retained local logs:

- `cargo build --locked --bin cassie`: passed before the batch and after the FILTER correction (`target/green-resume-build.log`, `target/green-filter-build.log`).
- `cargo test --locked --no-run --lib --test parser_types --test executor --test catalog_security --test compatibility --test sql_queries --test pgwire_extended`: passed compilation (`target/green-resume-test-build.log`).
- `python3 target/green-resume-run.py`: completed all seven selections; failures above are retained in `target/green-resume-results.json` and per-target logs.
- `cargo test --locked --lib sql::parameters::occurrence_tests::`: passed, 13 tests (`target/green-filter-unit.log`).
- `cargo test --locked --lib`: passed, 965 tests after the FILTER correction (`target/green-filter-lib.log`).
- `cargo test --locked --test pgwire_extended should_preserve_modifier_semantics_given_real_tables_and_scoped_scalars_when_simple_and_parse_phases_execute`: passed (`target/green-filter-wire.log`).
- Full workspace/all-target/all-feature pedantic Clippy: failed on three `large_enum_variant` errors in exported `QueryStatement`, `StatementRoute` and `RuntimeStatement` (`target/green-filter-clippy.log`). No lint suppression was added. The unapplied boxed-SELECT proposal requires the public-API direction specified by `AGENTS.md`.
- `cargo fmt --all -- --check` and `git diff --check`: passed after formatting the batch.
- `cntryl-tools validate-tests -f src/sql/parameters/occurrence_tests.rs` and the wire modifier matrix file: passed, 13/13 and 3/3 compliant.
- `target/test-inventory-tools312/bin/python tools/test_inventory.py --check`: passed after refreshing generated inventory; no unregistered Rust tests, unresolved module paths, duplicate registrations, unsharded targets or oversized files.

The complete repository test gate is still red. No issue completion, merge,
support promotion or production-readiness claim follows from this batch.

## Text-selection implementation batch, 2026-10-15

The next #803 slice implements text `split_part`, `substr`/integer `substring`,
`btrim`/`ltrim`/`rtrim`, and `strpos`/`position`, including SUBSTRING FROM/FOR
and TRIM direction/character-set syntax. It keeps the existing scalar-call AST
and TEXT carrier. Result slices use UTF-8 character boundaries without a retained
character vector; `strpos` reports int4 descriptors and binary values. Scalar
substring starts and split-field positions are signed; strict NULL controls and
invalid known signatures are separate tests.

The first coherent implementation run compiled four targets together, then ran
the complete library and parser/type targets and both affected string groups:

| Target | Passed | Failed |
| --- | ---: | ---: |
| Library | 967 | 0 |
| Parser/types | 296 | 77 |
| SQL query string group | 18 | 23 |
| Extended-wire string groups | 9 | 48 |

These 1,438 executed cases include 28 previously failing cases now passing and
eight new passing regression cases. No previously passing observed case failed.
One of the six corrected parser cases preserves bytea SUBSTRING argument
structure only; that AST observation is not bytea execution qualification.
Comparison inputs are the retained earlier baseline, resumed target results and
fresh six-case parser/53-case wire RED runs. See ignored local evidence
`target/green-text-comparison.json`, `target/green-text-results.json`, and the
`target/green-text-*-red.log` files.

Jev's signed-selection, keyword-boundary and parameter-phase gap probabilities
were 0.45, 0.46 and 0.43. The focused live PostgreSQL 18.6/Cassie probes checked
Unicode/nonpositive starts, negative and empty split fields, nested quoted
FROM/FOR/IN, four missing operands, strict NULLs and typed invalid overloads.
They confirmed two invalid typed-NULL calls originally bypassed overload
admission. The corrected wire regressions require `42883` in simple and
Parse/Flush phases, plus connection recovery. Error tests additionally cover
negative substring length `22011`, zero split index `22023`, invalid integer
text `22P02` and integer text overflow `22003`.

Native PostgreSQL PREPARE also confirmed all-unknown `substring($1 FROM $2 FOR
$3)` resolves to text/text/text, while `substr($1,$2,$3)` resolves to
text/int4/int4. The new parameter tests preserve this distinction and binary
result metadata. Regex SUBSTRING execution remains an explicit `0A000` pending
capability; its rejection is not a PostgreSQL execution pass. Formatting,
expanding transforms, regex families, other #803 functions and complete
resource/cancellation qualification remain open. No full-owner completion or
complete repository test gate is claimed.

Build and run commands are `cargo build --locked --bin cassie`, then
`cargo test --locked --no-run --lib --test parser_types --test sql_queries --test
pgwire_extended`, then `python3 target/green-text-run.py` against those compiled
binaries. A final private helper extraction addresses the batch's pedantic
function-length finding without lint suppression. The pre-existing three
exported AST enum-size findings remain subject to the earlier public-API
direction request.

Final readback reproduced the table above on the rebuilt source
(`target/green-text-final-results.json`). `cargo fmt --all -- --check`,
`git diff --check`, the touched-test validations and refreshed inventory check
pass. A test-only name correction then passed its two focused unit tests; the
assertion bodies and runtime code were unchanged. Final full pedantic Clippy
still fails only the three existing `large_enum_variant` findings in
`QueryStatement`, `StatementRoute` and `RuntimeStatement`
(`target/green-text-final-clippy.log`); the private function-length finding is
gone. No lint level was lowered.

The generated inventory counts **5,602 integration Rust instances**, not library
tests; the **967 library tests** are additional. Its 16 Python tests are a
separate harness. This corrects any reading of the integration-only inventory
as the complete Rust test total. Formatting, identifier parsing, expanding
functions, regex execution and other failing cells above remain pending for
later batches; the full repository gate is still red.

## Scalar text inspection batch after 4a6ada4f

The following #803 slice adds character/octet/bit lengths, signed LEFT/RIGHT,
ASCII/CHR, STARTS_WITH and its `^@` syntax, and non-strict NULL counts. It also
restores the existing embedded `Value::Int64` carrier for bounded int4-only
function arguments without retyping its parameter slot. Wire Parse with an
explicit int8 argument still rejects an int4-only overload with `42883`.

The coherent run executed 1,442 cases: library 968/0, parser/types 296/77,
SQL string and residual groups 25/19, and wire string groups 17/40
(passed/failed). Thirteen previously failing cases pass, with no observed
previously passing case failing; three behavioral regressions were added.
Native PostgreSQL 18.6/Cassie paired probes agree on values and SQLSTATE for
all seven scalar-text scenarios. The pinned reference's `right(text, INT_MIN)`
retains the whole text, matching the int32-negation/clipping path in
[REL_18_6 text_right](https://github.com/postgres/postgres/blob/REL_18_6/src/backend/utils/adt/varlena.c).
CHR preserves `22023` for negative codepoints and `54000` for zero, surrogates
and values beyond Unicode's maximum. These program-limit SQLSTATEs are not
claims about accounted-memory exhaustion.

Jev gap probabilities were 0.17 for the embedded/wire carrier distinction,
0.24 for scalar semantics and 0.27 for prefix precedence. Native probes include
comparison/AND precedence and NULL prefix/NULL-count controls. Evidence is in
ignored `target/green-text2-{results,comparison,oracle,probes,jev-response}.json`.
The build and batched test compilation passed. A final private prefix-parser
extraction fixes its pedantic function-length finding; the existing three
exported AST-size findings remain open. This is a checkpoint of a still-red
compatibility branch, not a completed #803 or full-repository gate.

Final readback: rebuild and batched test compilation passed. After the private
parser extraction, parameter occurrence tests passed 14/0; SQL and wire groups
retained 25/19 and 17/40. Full pedantic Clippy reports only the three known
large-enum findings. Formatting, touched-test validation (14/14 and 3/3),
inventory check and diff whitespace check passed. Inventory contains 5,604
integration instances; the 968 library tests are additional.

## Approved SELECT ownership change after bb594e9b

`QueryStatement::Select` and `RuntimeStatement::Select` now own
`Box<SelectStatement>`. The user approved this public Rust constructor change.
Parser construction, binder ownership transfers and direct test fixtures are
updated together. INSERT SELECT and derived sources transfer an existing box
instead of allocating a second one. Materialized projection planning clones the
underlying SELECT value. SELECT fields and SQL/wire contracts are unchanged.

The initial binary compile exposed 13 stale ownership/construction sites; the
all-target test compile exposed four additional test sites. After those updates,
`cargo build --locked --bin cassie` and `cargo test --locked --no-run` passed for
all test targets. The complete 968-test library run passed. Jev's specific
ownership/serialization/admission review returned a gap probability of 0.29;
source review confirms serde's SELECT payload remains the same structure and
existing pagination admission retains its conservative serialized-size bound.
Evidence is in ignored `target/green-select-boxing-*` logs and JSON artifacts.
Integration and analyzer results follow.

The required `cargo test --locked` run passed library 968/0, then stopped at
analytics 176/3. The three failures are stricter aggregate-admission conflicts
from the earlier batch: numeric aggregates over JSON, JSON extrema, and existing
Cassie vector extrema. They are not counted as passed. The next slice must
preserve the established vector extension, keep PostgreSQL JSON overload
rejection, and use explicit numeric casts for the mixed-JSON fallback witness.
PostgreSQL 18.6 independently returned `42883` for `MIN(json)` and accepted
`SUM/AVG/MIN/MAX(v::text::float8)` over numeric JSON scalars with
`2.5|0.8333333333333334|-1|2`.

Boxing-focused readback passed planner controls 126/0, derived sources 14/0,
relation identifiers 15/0, set operations 15/0 and null-safe predicates 49/0.
Complete parser/types retained 296/77 and the wire string groups retained 17/40,
matching the preceding checkpoint. These unresolved parity failures do not
become exclusions. No storage format or CI/release configuration changed.

Final full workspace/all-target/all-feature pedantic Clippy passed after fixing
16 previously masked integration-test findings: numeric literal separators,
constant-size chunk iteration and diagnostics for empty-row assertions. These
edits preserve asserted behavior. Rebuilt SQL/wire test targets passed compilation;
the two changed scalar text regressions passed. The broader affected wire
aggregate/lexical/text groups ran 19/7; failures include VALUES-source positive
controls, grouped primary-key dependencies and unsupported aggregate spelling.
They remain red and are not attributed to boxing or reported as completed.

Final formatting, all 11 touched-test file validations, inventory refresh/check,
whitespace check and touched Rust file size audit passed. All touched Rust files
remain below 1,000 lines. No integration test instances were added or removed.
This checkpoint clears the full Clippy gate; the full test gate remains red.

## Aggregate extension and JSON boundary batch after fe266aa9

The required prior run exposed three red analytics cases. Preserve Cassie's
established vector MIN/MAX extension in shared aggregate admission, while
continuing to reject vector SUM/AVG/Boolean aggregates and JSON numeric/extrema
aggregates. Numeric JSON fallback fixtures now explicitly cast JSON through
TEXT to FLOAT, as PostgreSQL requires. The array-extrema test retains its exact
typed-array ordering assertions and now checks rejection of JSON MIN and MAX.
The mixed numeric fixture retains indexed/unindexed equality, exact values,
float8 descriptors and row-fallback counters; it does not gain implicit JSON
numeric coercion.

The first adversarial review returned 0.57 for extension admission and 0.44 for
fixture coverage. Focused execution exposed two additional defects: vector
rejection diagnostics displayed `unknown`, and computed aggregate arguments
were omitted from the row-fallback counter. The refinement formats the existing
vector OID family without pre-admission allocation and records declined
registered aggregate projections. Diagnostic capacity probes include dimension
2 and the maximum admitted dimension 32,767.

After refinement, the 33-case related batch ran analytics 3/0, SQL aggregate
semantics 22/4 and selected empty/false-filter wire admission 4/0. All three
original analytics failures and the new vector/JSON admission probe pass. At
that run, the four remaining SQL failures were string_agg ordering, grouping
sets, ordered-set rank aggregates and statistical aggregates. The #804 slice
now supports SQL-002's single-expression grouping-set form; broader grouping
sets remain pending with those other obligations.
Refined Jev probabilities were 0.21 for bounded vector diagnostics and 0.27 for
fallback metric scope. Native PostgreSQL 18.6 confirms numeric JSON cast values
`2.5|0.8333333333333334|-1|2`, four double-precision descriptors and `42883` for
both JSON extrema. Artifacts are in ignored `target/green-aggregate-compat-*`
and `target/green-aggregate-json-*-oracle.log`.

The refined binary build and compilation of all 34 Rust test executables passed.
The repeated required `cargo test --locked` run has passed all 968 library tests;
final integration/analyzer readback follows below.

Final ordered readback: library 968/0, analytics 179/0, backend CI 2/0 and
benchmark operations 16/0 passed. The full locked test command then stopped at
catalog/security 139/209; later targets were not run by that command. The
additional complete metrics suite passed 61/0. Full workspace/all-target/all-feature
pedantic Clippy, formatting, all four touched-test validations, inventory
refresh/check, whitespace and touched-file size audits passed. All touched
Rust files remain under 1,000 lines. Inventory now contains 5,605 integration
instances; the 968 library tests are additional. One SQL behavioral probe was
added. No CI/release configuration or durable scalar layout changed.

The current batch implements native flat VALUES sources under #800 using the
existing primitive carriers. The user authorized idiomatic public API changes;
`QuerySource::Values` retains row expressions as PostgreSQL's VALUES range-table
entry does. `Expr::try_visit_children_mut` provides the matching mutable child
visitor. No stored scalar or collection layout changes.

The live #795 alias prerequisite is closed; its implemented alias foundation is
consumed by this slice. #793 remains open for extended scalar/wire/storage
contracts. Existing primitive identities are already specified by
`docs/type-contract.md`; this slice introduces none of #793's new families.
It does not claim either broad prerequisite or #800 complete. Standalone,
FROM and CTE VALUES, per-column supported primitive common types, aliases, ordering,
pagination, descriptors and typed parameters form the bundle. Functions and
subqueries inside VALUES, correlated LATERAL, nonprimitive casts, NUMERIC/new
carriers, row constructors, USING joins and TABLESAMPLE remain pending and
unsupported. Native character probes exposed shared #796 gaps in padding and
unconstrained bpchar typmods. Unconstrained CHAR VALUES columns deliberately
reject 0A000 before output while that shared type path is pending; the native
positive probes remain failing obligations, not exclusions. Existing bounded
character casts retain their preexisting support boundary. Full PostgreSQL
expression parity remains a dependency obligation.

Initial batch evidence: parser 296/77, selected SQL 5/17 and wire 10/10. The SQL
selection includes row constructors and USING joins outside this slice. Probes
confirmed parser token shortcuts bypassed postfix casts, native-source result
metadata was absent, and alias-count diagnostics exposed a private carrier.
Those defects are refined in this batch. Native PostgreSQL 18.6 also confirms
mixed VARCHAR typmods must be removed without truncating the longer row.
Jev's first parameter gap probability was 0.66 and allocation gap was 0.30;
focused probes cover binary BIGINT, incompatible columns at Parse, differing
VARCHAR typmods, oversized CHAR during planning, nonprimitive nested CHAR array
admission, result-row limits versus aggregate inputs, owner release and recovery.
The refined review returned 0.34 for primitive parameter/expression admission
and 0.20 for bounded evaluation. Its focused live probe confirmed the shared
arithmetic gap: Cassie returned 2147483648 for an INT4 overflow inside VALUES,
while PostgreSQL 18.6 rejects it. Arithmetic row expressions now reject before
output pending the shared #796/#800 operator implementation, and the native
positive probe remains red. No valid expected value or descriptor was weakened.

A 1 KiB library admission regression exposed extra retained frame size from
caching child counts. The refinement keeps the original frame size and obtains
rectangular VALUES child counts with checked constant-time multiplication;
row widths are checked once with cancellation checks. Malformed manual AST
sources receive a bounded syntax diagnostic instead of an indexing panic.
Final ordered validation and current-head evidence follow after execution.


VALUES checkpoint readback: the final ordered binary build passed. The full
locked test command passed library 968/0, analytics 179/0, backend CI 2/0 and
benchmark operations 16/0, then stopped at catalog/security 139/209. Later
integration targets were not run by that command. The independent complete
parser run remained 296/77. The 49-case related SQL/wire run was 30/19; within
its 23-case VALUES group, 20 pass and three native character/arithmetic probes
remain red under #796/#800. Fifteen existing selected failures are now green;
nine probes were added, six passing and three retaining pending obligations.

Full workspace/all-target/all-feature pedantic Clippy and formatting passed.
The touched-test validator required one naming refinement to express the
binary BIGINT case as one behavior; its rebuilt exact wire case passed. All
four touched-test validations now pass 27/27. Inventory refresh/check reports
5,614 integration instances plus 968 library tests; no unregistered tests,
module errors or touched Rust files above their limits. No test expectations,
resource limits or lints were weakened. Build/test/review artifacts are in the
ignored `target/green-values-*` files. This is a reviewable incremental
checkpoint, not completion of #800 or a repository-wide green result.

The next bundle is existing-width integer input and arithmetic under #796/#800.
The selected #792 dialect and #752 current-type contracts already specify the
16/32/64-bit families used here; the broader #792/#793 integration obligations
remain open. No new logical storage family, schema layout or migration is
introduced. A borrowed shared parser will serve text Bind and integer casts;
resolved integer operators will retain their native width directly in the AST,
without adding cast-wrapper nesting. Native PostgreSQL 18.6 `numutils.c` and
local probes pin radix prefixes, separators, ASCII whitespace, signed minima,
syntax/overflow precedence and INT2/INT4/INT8 result types. Existing scalar
extensions outside this finite input/operator slice are preserved. Bare
operator-inferred parameters and complete coercion/signature resolution remain
pending; SQL runtime cast diagnostics qualify the error state with bounded
messages, while wire input retains the existing detailed diagnostic path.


### Existing-width integer input and arithmetic checkpoint

The integer bundle introduces explicit `IntegerWidth`/`IntegerOperation` AST
metadata and checked arithmetic over existing INT2/INT4/INT8 carriers. VALUES
resolves operations before common-column coercion, preserving native overflow,
integer division, exact BIGINT values, NULLs and output descriptors. Typed
negation preserves postfix-cast-before-sign precedence. Public AST callers,
visitors, parameter summaries, structural identity, normalized cache shapes,
constant planning and pagination consume the selected width directly.

The shared borrowed integer parser handles native radix prefixes, separators,
signed minima and ASCII whitespace, including vertical tab. Text Bind validates
UTF-8/NUL first and returns native 22021; integer syntax/range then return 22P02
or 22003 with native priority. Runtime cast error strings remain bounded and
qualify SQLSTATE rather than full input echo. Existing decimal-only JSON casts
are preserved. Stored scalar encodings and SQL-string catalog definitions are
unchanged; no workflow, release or version configuration changes are included.

Test-only red runs failed all four initial SQL and three initial wire cases.
Focused probes then exposed JSON radix broadening, UTF-8 priority, vertical-tab
whitespace and signed postfix-cast precedence. Each confirmed defect was fixed;
positive/rejection probes remain in the native Rust integration suites. The
last related run was 77 passing/21 failing across 98 cases, including 29 library
and two float-cast parser probes. All 14 new SQL/wire functions pass. Remaining
selected failures concern CHAR, row constructors/USING, aggregate behavior and
previously pending temporal/array receive paths; none were weakened or skipped.

Jev's first implementation review returned parser/operator gap probabilities
0.54/0.36; native UTF-8 and parameter/domain probes led to fixes. Its refined
0.56/0.31 review triggered whitespace/postfix-sign probes, also fixed. The last
0.64 parser/0.30 operator review triggered another 20 native raw-wire cases for
ASCII/control whitespace, uppercase radix prefixes, decimal separators,
overflow/tail priority and malformed multibyte UTF-8. Rust wire cases cover the
distinguishing inputs, rebind recovery and signed overflow under a wider VALUES
column; all pass. The carrier/conversion gap was 0.22. Advisory probabilities
are not correctness proof. Full signature/coercion resolution, nonliteral unary
operators, new NUMERIC carriers and CHAR remain broader #792/#793/#796/#800
obligations. No owner is closed by this finite checkpoint.

Build/test/review evidence is under ignored `target/green-integer-*` files.
Final ordered validation results are recorded below after command completion.


Ordered checkpoint validation: `cargo build --locked` passed; the full locked
run passed library 977/0, analytics 179/0, backend CI 2/0 and benchmark
operations 16/0, then stopped at catalog/security 139/209. The separate full
parser target remained 296/77. The expanded final integer SQL/wire runs passed
6/0 and 8/0. The post-Clippy-refactor rebuild and complete library rerun also
passed 977/0. Clippy requested a unary-identity helper, equivalent walker-arm
merges and a statement semicolon; these are behavior-preserving cleanups.

Touched tests validate 23/23 across the two new unit files and two integration
modules. Inventory refresh/check reports 5,628 integration instances plus 977
library tests, no unregistered test/module findings, and no touched Rust size
violations. Full workspace/all-target/all-feature pedantic Clippy and formatting
results are recorded on the PR after the final pattern-format cleanup. This
checkpoint does not establish a green full suite or completion of #796/#800.

Final full pedantic Clippy and formatting passed. Clippy also refined empty-row
assertions to print mismatching values; both final integer integration modules
were rebuilt and rerun (wire 8/0, SQL 6/0). All four final validators passed
23/23, and the final regenerated inventory check passed. No test limits,
expectations, lint levels or broader pending obligations were weakened.


The next #800 bundle covers constructor-to-constructor row predicates over
existing scalar types: equality/inequality, lexicographic ordering, NULL rules,
distinctness, arity, prepared-wire descriptors and bounded traversal. Primitive
foundations are committed at 0641d0fe (977 library tests and 14 integer SQL/wire
functions pass); #795 is closed. #793 remains a wider composite/scalar integration
owner. These predicates need no composite stored carrier or format decision.
An explicit boxed RowComparison AST payload preserves scalar AST size and
selects a dedicated predicate enum. Whole-record and qualified-star row paths,
row output codecs and row-subquery #762 remain pending distinct obligations.
Native PostgreSQL 18.6 probes pin ordinary zero-field comparison as 0A000,
zero-field distinctness as Boolean, unequal arity as 42601, and eager constant
folding versus runtime pair short-circuiting. Jev selected this explicit
predicate design at 1.0 and prioritized arity probes. Six existing constructor
tests are red; twelve new SQL/wire regressions are authored before implementation.


### Primitive row-predicate checkpoint

Constructor equality/inequality, lexicographic ordering and distinctness now
use a boxed `RowComparison` AST with a dedicated operator enum. The result is
an existing Boolean value; there is no stored composite carrier or record
wire codec. Shared visitors, parameter admission/inference, source and output
types, expression identity, grouped projection keys, EXISTS/pagination
traversal, diagnostics and dependency inspection consume the new shape.
Native integer fields use the same checked signature resolver as VALUES.
Explicit FLOAT8 operands select floating comparison; exact NUMERIC operands
and extended-family/whole-record/star/subquery obligations remain pending.

Six existing constructor tests started red and now pass. The first twelve new
SQL/wire functions were 0/8 SQL and 1/3 wire: the apparent arity pass reported
a generic syntax error, so it now checks the intended native diagnostic.
Focused tests exposed eager constant errors for ordering, conditional-constant
pruning and missing INT2/INT4 field overflow. Native probes guided each fix.
Table-backed fixtures distinguish runtime variables from single-row VALUES,
which PostgreSQL can inline; the initial VALUES fixtures were corrected against
the oracle rather than used as false evidence of runtime laziness.

PostgreSQL 18.6 retains a multi-field RowCompareExpr after folding its inputs;
a singleton lowers to a scalar operator. Native CASE/CAST/NOT/nested-row probes
pin that distinction, including division errors in otherwise dead branches.
Equality/distinctness follow AND/OR constant pruning, including CASE, COALESCE,
immutable text functions and Boolean/BETWEEN expressions. The thirteen-case
native conditional matrix passes in Rust. Runtime partial evaluation shares
the original immutable-function set with constant planning. Text row fields
retain lexical identity rather than timestamp-like normalization.

Jev's first review was 0.75 planning / 0.58 typing / 0.43 resources. Native and
Rust probes cover conditional pruning, independently inferred INT4/BOOL slots,
repeated-slot 42P08, later invalid-field 42883, native 1664-field/54011 admission,
128 KiB memory rejection and owner release, fresh cancellation recovery and
Boolean view descriptors before/after restart. The original 1 KiB parameter
frame budget also passes. Refined review chose source-constant binding as the
next batch (0.99), with primitive-kernel gap 0.50 and owner gap 0.22. The
required extra kernel probe checks multi-field ordering opacity through
CAST/NOT/nested CASE and contrasts nested equality pruning; these pass.

A new native error-priority probe remains red under #800:
`SELECT ROW(a, 1/b) < ROW(a+1, 0) FROM (VALUES (1,0)) AS v(a,b)` must raise
22012 after PostgreSQL inlines the source constants. Cassie still treats these
columns as runtime variables. This is the next coherent source-binding bundle;
it is neither an exclusion nor a passing cell. Whole-record comparison,
qualified-star expansion, USING and character behavior remain visible failures.
The existing persisted-null-safe-definition boundary also covers RowCompare,
with its original diagnostic; its separate compatibility-law decision remains
pending. Native ordering-view SQL persists and rebinds across restart.

Ordered validation: build passed; the full locked run passed library 978/0,
analytics 179/0, backend CI 2/0 and benchmark operations 16/0, then stopped at
catalog/security 139/209. Parser remains 296/77. The post-cleanup complete
library rerun passed 978/0. The final related selection is 109/16 across 125
cases: library 30/0, float-cast parser 2/0, SQL 42/5 and wire 35/11. Of the 26
new SQL/wire functions, 25 pass and the source-constant probe is red. The
complete null-safe target passed 49/0 after extending its definition guard.
Full workspace/all-target/all-feature pedantic Clippy and formatting passed.
All four touched-test validators passed 43/43. No expectation, resource cap or
lint was weakened. Final inventory and push readback follow this checkpoint.
Artifacts are ignored `target/green-row-*` files; #800 and full-suite gates
remain open.

Final row inventory refresh/check passed: 5,654 integration registration
instances plus 978 library tests, no unregistered modules/tests and no touched
Rust size violations. Final validators remain 43/43. Full pedantic Clippy,
formatting and whitespace checks passed after the final nested-planning probes.
The exact native oracle reports PostgreSQL 18.6 (Homebrew).


The next source-constant bundle starts from pushed 90df9c30 and the retained
#800 single-row VALUES/22012 red probe. Native probes also require pruning
unused source arithmetic, preserving multi-field row-ordering opacity, folding
row equality through identity casts, and scalar AND/OR constant pruning. The
plan propagates original typed expressions after binder/parameter analysis,
preserves names/types and unused supplied slots, and removes the eligible
one-row source after consumer substitution. Eligibility is initially direct
VALUES or one alias wrapper, without source parameters/functions/CHAR or
consumer EXISTS/window/set boundaries. These wider forms remain obligations;
no stored type/layout or durable null-safe law is selected. Checked clone/name/
Vec admission precedes mutation, and cancellation is polled during traversal.
The same source pass may serve SELECTs inside INSERT once their ordinary
parameter/type binding is complete. Related Boolean execution shares the
semantic constant evaluator; private fold metadata must preserve pruned scalar
constants. Jev selected AST propagation at 1.0 and prioritized alias/parameter/
scope probes. Eighteen new SQL/wire functions are authored before implementation.

Source-constant implementation now propagates the original bound AST, retaining
row-ordering opacity and checked integer widths, and shares semantic constants
with scalar Boolean execution. It skips unused cells without evaluating them.
The initial new modules were SQL 8/6 and wire 3/1; the retained source-inline
22012 probe was also red. The green modules pass SQL 15/0 and wire 4/0, including
three nested CASE/row/IN clone-budget rejection and recovery probes. All 11 row
wire tests now pass. A complete library run exposed a runtime Boolean type-gate
regression for direct AST callers; the gate was restored and the subsequent
complete library run passed 978/0. Native type checking is not pruned away.
Jev's scope/constant/ownership gap probabilities were 0.15/0.14/0.40; the
ownership result prompted the nested clone probes. Parameters and functions in
source cells, CHAR, nested scope/window/set boundaries and DML inlining remain
pending rather than receiving an eager or lossy substitution. No persistent
format or stored type identity changed. Artifacts are `target/green-source-*`.

The ordered source checkpoint built successfully. `cargo test --locked` passed
library 978/0, analytics 179/0, backend CI 2/0 and benchmark operations 16/0,
then stopped at catalog/security 139/209, unchanged from the row checkpoint.
The final related selection passes 132 and fails 15 across 147 cases: library
30/0, float-cast parser 2/0, SQL 60/5 and wire 40/10. New modules pass 19/19;
all 26 row SQL/wire functions pass. The remaining selected cases belong to
whole-record/qualified-star/USING, CHAR, aggregate or scalar-receiver owners.
Clippy found an oversized new helper and two missing semicolons; projection
rewriting was extracted into its own helper, without changing behavior. The
post-extraction build and complete library rerun pass 978/0. Final focused,
full Clippy, formatting and validator results follow below.

Final source checks pass: full workspace/all-target/all-feature pedantic Clippy,
formatting, both test validators (19/19), refreshed inventory check, touched Rust
size limits and whitespace. Inventory contains 5,673 integration registrations
plus 978 library tests. The post-extraction related selection remains 132/15.
The full-suite catalog failures remain open; no expectation or limit was weakened.

The next scalar-presentation bundle starts from pushed 6f1bf809. Existing
scalar-receiver recovery tests expose TIMESTAMP text output in internal form.
Native 18.6 probes also distinguish FLOAT8 scientific thresholds and exponent
spelling. Scope: existing OIDs 701/1114, default shortest float and ISO temporal
output, SQL text casts, parameters, derived expressions, persistence/recovery
and resource/cancellation siblings. Cast ASTs will retain optional bound source
type metadata, preserving it through visitors/keys and shared constant/runtime
execution. This is transient type information, not a new stored type/layout or
normalization policy. Jev selected bound source types at 1.0; source-scope and
midpoint gaps 0.60/0.71 require focused probes. New temporal families, full
DateStyle/extra_float_digits behavior, exact NUMERIC, infinity/large-year and
legacy-array identities remain separate owner obligations. No CI/version edits.

Scalar rendering probes reproduced SQL 2/6 and wire 0/6 before implementation.
The FLOAT8 prototype found 12 differences between ordinary Rust shortest
spelling and native output, plus 46 asymmetric-interval cases when choosing
only the nearest decimal. Exact dyadic midpoint checks, ties-to-even and the
shortest valid alternative now match 114,275 sampled native bit patterns.
Fixed Rust runtime fixtures contain 178 float, 12 timestamp and eight time
witnesses and exercise text/binary inputs, text/binary outputs and SQL casts.
No expectation is generated by Cassie's formatter. FLOAT8 Bind syntax/range
states and valid VT/whitespace/non-finite forms were also verified through
actual native Parse/Bind/Execute frames. Shared decimal/non-finite input parsing
preserves 22P02/22003 and recovery; complete native hex input remains a #796
obligation. SQL input errors use typed FloatInputError/CassieError propagation,
avoiding an unrelated generic 22000 string fallback found by the wire-core run.

Cast metadata is populated only for TIME, TIMESTAMP or proven FLOAT8 output
conversions. Uncast NUMERIC-literal proxies keep their earlier scalar output path. Visitors,
EXISTS reconstruction, structural identity, grouped keys and feedback shapes
preserve selected source identity; clone accounting includes optional types.
Native storage normalization, type identities, layout and CI remain unchanged.
Jev review reported source/float/resource gaps 0.54/0.17/0.23. The source result
prompted correlated EXISTS, grouped-key, aggregate and actual persisted-view
rebind probes. Existing supported assignments seed these fixtures; unsupported
CASTs in INSERT and unknown-text-to-FLOAT8 assignment are separate obligations.


The final source review also reproduced an explicit NUMERIC-versus-FLOAT8 text
regression caused by the legacy alias mapping. `CastIdentity` now retains
NUMERIC/DECIMAL origin, and the output-provenance analyzer excludes those casts
from proven FLOAT8 sources. The native paired result is decimal text versus
`1e+20`; the regression test passes after this fix. Exact NUMERIC, stored legacy
schema aliases and the new-v3 carrier/codec/migration remain #793/#812 work.
Integer-to-FLOAT8 assignment was probed through SQL text and both wire result
formats; existing storage canonicalization preserves the declared FLOAT8 value.
The correlated EXISTS, grouped, scalar aggregate and persisted-view restart
probes pass. A full wire-core run improves from 72/12 to 73/11 after typed FLOAT8
range errors; remaining cases belong to NUMERIC/COPY/CHAR or zoned temporal work.

The final complete ordered source gate passed build, library 978/0, analytics
179/0, backend CI 2/0 and benchmark operations 16/0, then stopped at the unchanged
catalog/security 139/209. Related selection: 161 passed/16 failed across 177
functions (library 30/0, parser 2/0, SQL 77/7, wire 52/9). The 29 new functions pass 26/3:
temporal VALUES admission (#800), out-of-range numeric-literal ASTs (#812/#796)
and quoted unknown-literal input errors before pruning (#796/#800) are retained
red probes for the next batch. Native probes distinguish quoted literal input
from numeric conversion and from explicit TEXT casts. These obligations remain
in the default suite. No expectations, budgets or lint levels were weakened.
Clippy cleanup replaces generic defaults with explicit CastIdentity values and
extracts EXISTS predicate execution into a focused module. Final post-cleanup
focused/library/core checks, full Clippy, formatting and validators follow.

Final post-cleanup results: library 978/0, related selection 161/16 and complete
wire-core 73/11. Full pedantic Clippy and formatting pass. Initial test policy validation passed
123/126, flagging three action names with `_and_`; those names were corrected
to describe the single identity invariant, with assertions unchanged. The
premature all-pass note was corrected before commit. Final policy/inventory
receipts follow. Touched Rust size and whitespace checks pass. Pure EXISTS extraction preserves the reviewed context and reservations.
Finite FLOAT8 wire rendering is restricted to declared OID 701; other carriers
keep their existing finite output path. No semantic expectation was weakened.

Final test-policy result: 126/126 pass across 15 touched files, including
29/29 new functions after the three name corrections. Final formatting,
refreshed inventory check (5,702 integration registrations/978 library tests),
size and whitespace checks pass. Runtime assertions and pending probes are unchanged.

## Exact transient integer literal batch after b08fbf84

This #796/#812 prerequisite preserves overflowing decimal integer tokens as an
exact, transient numeric AST leaf, distinct from quoted unknown strings. The
bundle covers decimal digits and separators, radix magnitudes through u64,
explicit existing integer/FLOAT8/TEXT/VARCHAR conversions, native signed-minimum
syntax, CASE/Boolean/COALESCE pruning, single-row VALUES substitution, primitive
wire descriptors and both result formats, range errors and connection recovery.
Native PostgreSQL 18.6 witnesses are recorded in ignored
`target/green-literals-native.json`. Quoted input validation before pruning,
NUMERIC storage/output, decimal arithmetic, and radix magnitudes beyond u64 remain
with their existing owners. No stored format or codec is introduced here.

Implemented `Expr::NumericLiteral(String)` as exact normalized integer syntax,
including the lifted positive magnitude of postfix INT8 minimum negation.
Primitive casts share borrowed integer/FLOAT8 input and exact text conversion;
VARCHAR truncates explicit numeric text conversion to its declared width.
Structural traversal/equality, parameter counting/admission, feedback shapes,
expression keys, scalar accounting and source-clone admission preserve the leaf.
No numeric stored carrier, catalog format, or codec was added. A typed parser
`NumericRange` classification reports native 22003 for more than 131,072
significant decimal digits, even inside an otherwise dead conversion.

Red evidence on b08fbf84: the initial SQL module passed 1/10 and wire passed 0/4;
large tokens failed at parsing. The first implementation exposed explicit
VARCHAR truncation and review found a VALUES coercion escape. Jev's escape
probabilities 0.66 then 0.74 prompted focused probes: an uncast VALUES numeric
incorrectly returned TEXT OID 25, while native PostgreSQL reports NUMERIC OID
1700. The regression now requires explicit unsupported admission until #812;
VALUES validates new leaves before generating implicit casts. The same selected
cast gate rejects legacy NUMERIC/DECIMAL materialization of exact leaves.
Final focused advisory gap probabilities were 0.25 for that escape and 0.15 for
primitive cast rejection; passing Rust behavior remains the evidence.

Final batch evidence:

- New SQL module: 14/14 passed. New wire module: 7/7 passed. New AST identity and
  serde regression: passed. All 22 new functions are green. The earlier retained
  large-integer CASE pruning probe also passes.
- Actual native extended protocol preserves supplied trailing parameter OIDs
  23/20 and TEXT OID 25 through statement/portal descriptions and binary output.
  Native sign, radix, digit-limit, cast/error and pruning witnesses are retained
  under ignored `target/green-literals-native*` artifacts.
- `cargo build --locked --bin cassie`: passed, including after cleanup.
- `cargo test --locked`: library 979/0, analytics 179/0, backend CI 2/0 and
  benchmark operations 16/0 passed; stopped at catalog/security 139/209. Later
  targets were not executed by that command. This final broad run includes the
  VALUES fix and precedes removal of one redundant `None` match arm for Clippy.
- `cargo test --locked --lib --test sql_queries --test pgwire_extended --test parser_types --test pgwire_core --no-run`:
  passed after cleanup.
- Compiled-binary related selection after cleanup: 184/15 across 199 functions
  (library 31/0, parser 2/0, SQL 92/6, wire 59/9). The remaining failures have their
  existing subsystem owners. New modules and the retained numeric pruning probe
  are included.
- Complete parser target: 298/75. Complete wire-core target: 73/11. The earlier
  296/77 parser checkpoint predates the already implemented FLOAT8/temporal
  presentation fixes; its two now-passing tests are not attributed to this batch.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed after removing the redundant arm; no lint suppression was added.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: 25/25 across the two new modules and
  touched AST test module.
- Inventory refresh/check, touched Rust file limits and whitespace checks:
  passed. Inventory records 5,723 integration registration instances; the library
  has 979 tests. These are registration counts, not passing-suite claims.

The two retained scalar-presentation probes still red are temporal VALUES
admission (#800/#814) and quoted unknown-literal input errors before pruning
(#796/#800). NUMERIC storage/output/arithmetic, implicit exact-numeric coercion
and radix magnitudes beyond u64 remain pending under #812. Full parity, external
qualification and an entirely green repository are not claimed.

## Primitive quoted-input binding batch after e179ded8

This #796/#800 batch validates quoted unknown INT2/INT4/INT8, FLOAT8 and BOOLEAN
inputs in binder reference order and before VALUES coercion/pruning. It preserves
pruning of explicitly typed TEXT conversions, exact numeric AST provenance,
integer extrema and native Boolean spellings. It also qualifies BOOLEAN text
Bind syntax errors and recovery. Parameter admission continues to precede binder
validation; broader cross-phase parameter/function/type error precedence remains
pending. Temporal VALUES admission requires its own native input grammar work,
including precision and offsets, and is not widened by this primitive validator.

Implemented borrowed unknown-literal validation for selected scalar casts in
reference order, before VALUES coercion, and in nested Boolean binding contexts.
Quoted unknown input is validated even when CASE, Boolean operands, unused
VALUES cells, EXISTS targets or CTEs later disappear. Explicitly typed TEXT
conversions retain planning/runtime pruning, and exact NumericLiteral leaves
retain their separate provenance. NUMERIC/DECIMAL aliases do not invoke the new
FLOAT8 input validator. New typed BooleanInput and UndefinedColumn public errors
retain native 22P02/42703 classifications; missing-column display text remains
compatible. Boolean text input now uses native ASCII whitespace (including VT),
and text Bind rejects invalid spellings with semantic 22P02 rather than 08P01.

Red evidence: initial SQL 4/6, wire 0/5; expanded baseline SQL 6/12, wire 2/11.
After implementation/refinement, the final new modules report SQL 13/13 and wire
11/12. Thus 24 of 25 new functions pass. The retained prior quoted-input pruning
probe also passes. No expectations were weakened or tests ignored. One existing
wire-core expectation for Boolean `o` changed from 08P01 to native 22P02, verified
against real native Parse/Bind/Execute frames.

The new Parse+Flush probe intentionally remains red under #794/#796. Native
returns 22P02 before ParseComplete and ignores Describe until Sync. Cassie's
handle_parse currently performs parameter admission and stores the raw AST,
deferring semantic binding to Describe/Execute. The next Parse analysis slice
must resolve names/scopes and validate input before prepared-statement storage,
retain checked parameter metadata/ownership, and leave numeric conversion
folding in its later native phase. Calling the physical planner during Parse
would incorrectly move arithmetic/conversion errors into that phase. Parameter
admission still precedes binder validation; broader mixed-error precedence is
also pending. Temporal VALUES remains with #800/#814: precision, 24-hour TIME,
DateStyle and ignored TIMESTAMP offsets need their own input-law qualification.

Native PostgreSQL 18.6 matched 120 primitive cast/pruning witnesses (20 invalid
inputs across six contexts), 50 Boolean prefix/control-character witnesses,
text Bind error/recovery frames and the new failing Parse phase contract.
Ignored `target/green-inputs-native*` files retain these oracle observations.
Jev's initial EXISTS gap 0.50 and typed-TEXT gap 0.30 prompted focused EXISTS/CTE,
empty-result and typed-conversion pruning probes, which pass. Its final Boolean
candidate probability 0.47 and overall new-gap probability 0.46 prompted the
50-case native/SQL input and 100-cycle Rust wire-format matrix; no Boolean
mismatch was confirmed. These are advisory judgments, not correctness proof.

Validation:

- `cargo build --locked --bin cassie`: passed, including after test-only cleanup.
- `cargo test --locked`: library 979/0, analytics 179/0, backend CI 2/0 and
  benchmark operations 16/0 passed; stopped at catalog/security 139/209. Later
  targets were not executed by that command.
- Targeted locked compilation passed. Related compiled-binary selection: 209/15
  across 224 functions (library 31/0, parser 2/0, SQL 105/6, wire 71/9). All 25 new
  functions and the retained quoted-input pruning probe are included.
- Complete parser target: 298/75. Complete wire-core target: 73/11.
- Full workspace/all-target/all-feature pedantic Clippy, formatting, touched-test
  validators, inventory, touched Rust file limits and whitespace results are
  recorded in the final checkpoint below.

Full PostgreSQL parity, Parse-phase qualification, external readiness and a fully
green repository remain incomplete.

Final checkpoint: full strict Clippy passed after test-only readability/chunk
iteration fixes. Formatting passed; touched-test validation passed 31/31 across
three files. Final new-module reruns report SQL 13/0 and wire 11/1, preserving the
Parse+Flush expectation. Inventory refresh/check, touched Rust file limits and
whitespace checks passed. Inventory records 5,748 integration registration
instances plus 979 library tests, not an all-green result.

## 2026-10-16 — Larger Parse semantic-analysis batch

The new wire admission module runs checked parameter occurrence/type analysis,
then shared SELECT/EXPLAIN SELECT binding, then existing final slot validation.
Semantic errors occur before ParseComplete and prepared storage. The physical
planner is not called: division by zero, numeric narrowing and explicit TEXT
conversion errors remain beyond Parse. The raw submitted AST is retained for
later catalog/session rebinding.

A borrowed, checked footprint walk admits the temporary raw analysis copy
before cloning; its lease spans binding and releases on success or failure.
This covers the new raw clone, not legacy binder catalog/name/type allocations
or subsequent expansion. Cancellation, deadline and nesting are checked during
preflight. There are no format, storage-layer or CI changes.

Red baseline: 11 failed and five passed among the first 16 new wire functions.
The final batch has 31 passing functions: 24 wire and seven resource/ownership
functions. The previous primitive Parse+Flush probe also passes, making that
batch's primitive modules 25/25. New matrices cover 160 invalid primitive
Parse cases, ignored messages until Sync, failed-name reuse, unrelated named
statement survival, unnamed recovery, close/reuse, binary binding, inferred
Boolean parameters, trailing-slot checks, raw VALUES rebinding and deferred
constant evaluation. Pinned native PostgreSQL 18.6 independently matched all
160 errors plus three Parse-success/Describe-success/Bind-error sequences.

Sibling coverage exposed an initial BOOL_AND arity regression (42601 instead
of 42883). Keeping shared occurrence analysis before binder validation restores
its original expectation. Jev selected the admitted-copy approach; clone-gap
probabilities 0.57 and 0.68 prompted a seven-shape large-payload admission probe.
With raw SQL cleared, nested function/row/CASE/alias/identifier/window/VALUES
payloads exceed the 16-KiB budget before clone allocation and leave zero
peak/live reservation. The probe passes; full legacy binder resource behavior
remains a separate boundary. Jev is advisory rather than proof.

Validation before mechanical Clippy extraction:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 986/0, analytics 179/0, backend CI 2/0 and
  benchmark operations 16/0 passed; stopped at catalog/security 139/209.
  Later targets were not run by that command.
- Related compiled-binary selection: 241/14 across 255 functions, comprising
  library 38/0, parser 2/0, SQL 105/6 and wire 96/8.
- Complete wire core: 73/11, matching the previous checkpoint.

The full run contains the final semantic ordering. Subsequent mechanical
extraction moves that sequence to a focused wire module to satisfy the
100-line Clippy function limit; a missing statement terminator is also fixed.
Final build and locked affected-test compilation passed after the extraction.
The related selection remained 241/14. A test-only slice-borrow cleanup passed
Clippy; matrix helpers were then separated into Act and Assert phases for the
test validator. The final new wire module rerun passed 24/24; the preceding
combined new/primitive/aggregate-arity rerun passed 37/37. Full strict workspace,
all-target, all-feature pedantic Clippy and formatting passed. Touched-test
validators passed 31/31 across two files. Inventory refresh/check, touched Rust
file limits and whitespace checks passed. Inventory records 5,772 integration
registration instances plus 986 library tests; these are not all-green counts.

Broader mixed error precedence, native Describe/Bind planning phases and the
existing PostgreSQL parity owners remain incomplete. The repository is not
fully green. Evidence is retained under ignored `target/green-parse-*` paths.

## 2026-10-16 — Larger Bind planning and recovery batch

SELECT and DML Bind now compile under registered cancellation and query controls,
then invoke the existing constant-planning checker before storing a portal or
sending BindComplete. Parameter count, formats and receivers retain precedence.
The validation path does not execute query rows or apply writes. Utilities such
as EXPLAIN defer their contained planning to Execute; EXPLAIN now reports
constant errors before formatting its plan. INSERT VALUES shares the expression
parser used by standalone VALUES, allowing arithmetic to reach its native phase.

Concrete parameter-free arithmetic selects the existing checked integer kernel.
Compiled-plan epoch 5 isolates earlier fallback signatures in L1 and persisted
L2 caches without deleting the old records or changing stored data formats.
Missing portal errors now use 34000; missing prepared statements retain 26000.
Existing portal assertions were corrected to that distinction after native
Describe/Execute observations, preserving their lifecycle/value assertions.
Wider prepared/portal lifecycle parity and parameter-dependent custom/generic
planning remain with #821.

The batch adds 40 functions: 34 wire and six near-code control/cache tests.
Initial red run: 4/16 across the first 20 wire functions. Final new wire module:
32/2; all six control/cache functions pass, giving 38/40 new functions green.
Two native expectations remain red without skips or weakened assertions:

- Parenthesized unary negation is rejected at Parse rather than reaching Bind
  (#794/#796).
- An unused SELECT CTE still executes and raises its constant error at Execute
  (#800/#801), despite being correctly skipped by the Bind constant checker.

Native PostgreSQL 18.6 matched 63 error-phase cases across default/text/binary
result formats, 14 pruning/result cases, five DML Bind-without-Execute controls,
portal/prepared name states, receiver/count precedence, explicit transaction
failure and EXPLAIN phase frames. Real Rust connections additionally verify
failed-portal absence and reuse, repeated cached-plan rejection, Sync recovery,
implicit-write rollback and successful DML Bind without mutation.

Jev's triage question about the next phase/lifecycle risk selected portal
ownership at 0.82; native lifecycle frames and Rust recovery tests cover it.
Its adversarial question about a concrete introduced risk selected cache at
0.59 (overall gap probability 0.48). Focused L1/L2 probes reproduce the old
incorrect 2,147,483,648 result, then verify current Bind rejects 22003 and leaves
zero live query reservation while the old derived artifact remains readable.
Those probes pass. Jev remains advisory rather than correctness proof.

Validation:

- `cargo build --locked --bin cassie`: passed.
- Locked affected-test compilation: passed for library, parser, wire core/wire
  extended, SQL, storage/index and compatibility targets. Wire extended was
  rebuilt after a test-only missing-statement expectation correction.
- Final `cargo test --locked`: library 992/0, analytics 179/0, backend CI 2/0 and
  benchmark operations 16/0 passed; stopped at catalog/security 139/209.
  Later targets were not executed by that command.
- Related compiled-binary selection: 279/16 across 295 functions (library 44/0,
  parser selection 2/0, SQL 105/6, wire 128/10), including all 40 new functions.
- Affected portal/transaction/Boolean sibling selection: 51/1. Its remaining
  failure is the existing unsupported explicit-transaction TRUNCATE case.
- Complete parser: 299/74, improving one CASE/function-expression case from
  298/75 with no newly failing parser cases. Complete wire core: unchanged 73/11.
- Three renamed-test behavior probes pass. Existing TIMESTAMPTZ setup and
  persisted-operator grammar probes still fail; their bodies are unchanged.
- Full workspace/all-target/all-feature strict pedantic Clippy and formatting:
  passed. No lint suppression or CI/release configuration changes.
- `cntryl-tools validate-tests -f <path>`: 99/99 across 14 touched test files.
- The exact global CI command `cntryl-tools validate-tests`: 6,661/6,661 source
  tests compliant after five naming-only repairs. This is a static guideline
  gate, not 6,661 passing runtime tests.
- Inventory refresh/check, touched Rust file limits and whitespace: passed.
  Inventory records 5,806 integration registration instances plus 992 library
  tests, distinct from passing counts.

An intermediate full run was explicitly cancelled during analytics when the
INSERT parser defect superseded it; it is not counted as a completed gate.
The final complete run above includes all runtime changes. Ignored
`target/green-bind-*` files retain commands, native packets and test evidence.
Full PostgreSQL parity and a green repository remain incomplete.

## 2026-10-16 — CTE reachability and local EXISTS planning batch

Bind constant checks and Execute materialization now share a borrowed,
interruptible CTE dependency walk. Only reached SELECT definitions and their
transitive dependencies are inspected or materialized. Nested scopes resolve
names against the nearest visible definition; a nonrecursive CTE body sees
preceding definitions, while an admitted recursive body can reference itself.
The walk includes expression subqueries, joins, set branches, VALUES/table
function inputs and pagination/window expressions. Qualified catalog relations
do not become CTE references merely because their final name matches.
Unused CTEs still undergo semantic binding: missing relations and columns retain
native Parse errors. Non-SELECT definitions are conservatively retained; this
does not implement data-modifying CTE support or its dependency semantics.

The dependency stack and live masks reserve query memory before allocation.
Cancellation/deadline checks and a 128-level traversal bound remain active;
the returned mask owns its reservation until the caller releases it. Queries
without local CTEs avoid scratch allocation. Local WITH definitions inside
simplified EXISTS now undergo constant checks before irrelevant targets are
pruned. The previous batch's unused-CTE Execute failure is resolved; its unary
negation Parse expectation remains pending.

The batch adds 48 functions: 38 wire and ten scope/resource controls. Initial
red run of the first 24 wire functions was 9 passing / 15 failing. The expanded
38-function wire run was 31/7 before the local EXISTS correction. Final focused
results are wire 33/5 and controls 10/0, giving 43/48 new functions green.
Five native expectations remain explicit: two subqueries-in-VALUES cases reject
0A000, and three nested CTE outer-shape cases reject 42601 at Parse. These remain
with #796/#800/#801; the dependency-only controls pass but do not qualify the
complete wire behavior. No skips or weakened assertions were introduced.

PostgreSQL 18.6 matched 72 text/binary phase/value probes and two independent
unused-definition semantic Parse controls. Jev selected shared reachability at
1.0; its initial gap probability 0.61 prompted eight expression/scope probes.
Adversarial scope risk 0.63 and gap 0.39 prompted the same-name outer-read,
nested-unused, qualified-relation and recursive-cycle controls. The last review
still assigns gap 0.34; pending full nested-shape support stays linked to
#800/#801. Jev is advisory; executed evidence defines support.

A broader column-inference candidate was reverted after it admitted inputs whose
Describe phase timed out or lost enclosing relation metadata. Its superseded
focused run was cancelled; no partial binder change remains. Existing Parse
rejection and the failing native expectations are preserved. Ignored
`target/green-reachability-*` artifacts retain commands, native frames, review
questions, dispositions and validation logs.

Validation of the final runtime:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 1,002/0, analytics 179/0, backend CI 2/0 and
  benchmark operations 16/0 passed; catalog/security 139/209 stopped the run.
  Later targets were compiled but not executed by that command.
- Related compiled-binary selection: 323/20 across 343 functions (library 54/0,
  parser selection 2/0, SQL 105/6, wire 162/14). Comparing failure identities
  with the prior batch shows the unused-CTE expectation fixed, no prior passing
  case newly failing, and only the five added pending cases newly red.
- The broader SQL CTE-owner selection remains 0/31, with identical failure
  identities before and after this batch; MATERIALIZED/SEARCH/CYCLE and other
  complete-owner obligations remain open.
- Full workspace/all-target/all-feature strict pedantic Clippy and formatting:
  passed without lint suppression or CI/release configuration changes.
- Touched wire/unit validators: 48/48 compliant; the changed support fixture
  also validates. Global `cntryl-tools validate-tests`: 6,709/6,709 compliant.
  These are source guideline counts, not runtime passing counts.
- Inventory refresh/check, touched Rust file limits and whitespace: passed.
  Inventory records 5,844 integration registration instances plus 1,002 library
  tests, distinct from executed/passing totals. No benchmark timing is claimed.

Full repository green and PostgreSQL qualification remain incomplete.

## 2026-10-16 — Primitive unary operators and type propagation

The retained parenthesized unary-negation gap now reaches native Bind planning.
Numeric prefix signs have an explicit `UnaryNumeric` AST node. Parsing consumes
prefix chains iteratively, preserves postfix-cast precedence, folds direct
numeric constants and bounds combined operand/prefix depth before allocating
wrappers. Concrete integer minus binds to the existing width-specific checked
kernel; plus preserves its concrete operand type. FLOAT8 minus preserves signed
zero and infinity. Unresolved integer operands never receive a guessed BIGINT
runtime signature. Rebuildable compiled-plan epoch 6 invalidates earlier derived
plans; stored Value layouts are unchanged. The additive public AST contract is
recorded in [type-contract.md](../type-contract.md).

Raw unknown constants select FLOAT8 for unary plus and report 42725 for unary
minus. Known nonnumeric operands report 42883; invalid selected FLOAT8 input
reports 22P02. The selected type is visible to enclosing Boolean contexts and
parameter/CASE inference before unary rewriting. Explicitly cast parameters
retain their inferred OIDs and concrete operand signatures. Bare-parameter
operator defaults, parameter-dependent custom Bind planning, full NUMERIC and
interval/geometric/extension overloads remain with #793/#794/#796/#821.

The batch adds 77 functions: 61 wire, eight stored-row SQL and eight parser/kernel
controls. Initial wire red run was 2/39 among 41 functions. A signed-minimum
parser control reproduced 7/1, then became green after reclassifying the signed
literal carrier. Four additional native context controls reproduced 2/2: WHERE
+NULL was accepted and simple CASE inferred TEXT instead of FLOAT8 for a
parameter. Both failures are fixed through shared unary result-type inference
and bounded parameter-fold lookup. The 61-function wire module is now 61/0;
the eight SQL and eight parser/kernel controls also pass.

PostgreSQL 18.6 records 104 default/text/binary phase/value observations, six
independent Parse signature/input errors, eight explicitly cast parameter
observations and seven enclosing-context observations. Successful Rust wire
assertions use captured native payload bytes and check result OIDs/formats;
error controls verify Parse versus Bind and Sync recovery. Stored rows verify
all three integer-width overflow boundaries, filtering, aggregate arguments,
NULL propagation and repeated cache reads. Traversal, structural identity,
combined depth and IEEE sign controls cover the new AST node.

Jev selected the explicit AST design at 1.0. Its signed-minimum risk 0.90 prompted
four widening-before/after-negation probes. Adversarial gap 0.39, minimum risk
0.57 and parameter risk 0.40 prompted the widening controls and four explicitly
cast parameter probes. Additional source review found the two context failures
above; all four context controls now pass. Jev is advisory, not correctness
proof. Ignored `target/green-unary-*` artifacts retain requests, dispositions,
native packets and validation logs. No CI/release/version configuration or lint
suppression is introduced.

Final validation after extracting focused prefix-validation and BETWEEN-demand
helpers to satisfy the strict function-size limits:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 1,010/0, analytics 179/0, backend CI 2/0 and
  benchmark operations 16/0 passed; catalog/security 139/209 stopped the run.
  Later targets compiled but were not executed by that command.
- Related compiled-binary selection: 401/19 across 420 functions (library 62/0,
  parser selection 2/0, SQL 113/6, wire 224/13). All 77 new functions pass;
  prior failure identities show the retained unary-negation case fixed and no
  prior passing case newly failing. The preceding Bind batch's 40 functions
  are now all green. The five staged CTE expectations remain red.
- Complete parser: unchanged 299/74. Complete wire core: unchanged 73/11.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed. The earlier completed gate reported five Clippy defects; all were
  fixed without suppression, and the ordered gate was rerun on the final code.
- `cargo fmt --all -- --check`: passed.
- Touched wire/SQL/parser/kernel validators: 77/77 compliant; the new support
  fixture also validates. Global `cntryl-tools validate-tests`: 6,786/6,786
  compliant. These are source guideline counts, not runtime pass totals.
- Inventory refresh/check, Rust file limits and whitespace: passed. Inventory
  records 5,913 integration registration instances plus 1,010 library tests,
  distinct from executed/passing counts. No benchmark timing is claimed.

Both full test invocations completed through the known catalog failure; no full
run was cancelled. Full repository green and PostgreSQL qualification remain
incomplete. The oracle server reports PostgreSQL 18.6; the native executable
SHA256 is `db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`,
recorded in `target/green-unary-native-version.log` with the version readback.

## 2026-10-16 — Unary pagination signatures, planning and CTE reachability

This batch extends the existing primitive foundations for #796/#821 with 96
wire functions. PostgreSQL 18.6 supplies 48 Parse-error cases, 32 success cases
with text/binary results, eight Bind-overflow cases and eight adversarial
phase/reachability controls. The first 64-function red run reproduced 32/32.
The expanded run reproduced 50/38 before fixing parser admission order and
planning-time bound overflow. Final adversarial controls reproduced two unused
CTE execution failures; both are fixed. The focused wire pagination run is
98/0, including the 96 new functions and two existing controls.

Pagination now parses expression syntax before semantic integer admission.
Prefix signature checks borrow the operand and share the binder's operator
selection law: unknown minus reports 42725; typed nonnumeric prefix operands
report 42883. Typed integer/NULL controls preserve result OIDs and native
text/binary row bytes. General FLOAT8/NUMERIC pagination coercions and unknown
parameter overload defaults remain pending owner obligations, not exclusions.

Reached integer-bound constants are checked during planning, including constant
subtrees in parameter-dependent expressions. This reports arithmetic overflow
at Bind without applying negative LIMIT/OFFSET values, which still fail at
Execute with 2201W/2201X. Execution-time bound normalization uses the same
scope-aware CTE mask as constant planning/materialization; unused definitions
are not evaluated. Semantic validation continues to inspect all definitions.
The private rebuildable compiled-plan epoch is 7; stored Value layouts do not
change. No CI/release/version configuration is changed.

Jev selected shared validation at 1.0 and nested-query risk at 0.99; 16 nested
CTE/derived/EXISTS controls were added. Adversarial gap 0.55 prompted constant
subtrees with parameters, late negative bounds, used/unused CTE chains and
EXISTS probes. The unused-CTE failures above were corrected using the shared
reachability mask. Final advisory gap is 0.27. The native oracle version was
read back as PostgreSQL 18.6 (Homebrew, arm64 Darwin 25.6); ignored
`target/green-page-*` files retain exact SQL, native frames, red/green logs,
Jev requests/results and validation receipts. Jev is advisory, not proof.

Validation:

- `cargo build --locked --bin cassie`: passed.
- Two complete `cargo test --locked` runs finished through the existing
  catalog/security blocker: library 1,010/0, analytics 179/0, backend CI 2/0,
  benchmark operations 16/0, catalog/security 139/209. All 209 failure identities
  match the previous batch. Later targets compiled but were not executed by
  those full commands; neither full run was cancelled.
- Final `cargo test --locked --test pgwire_extended pagination`: 98/0, including
  all 96 new functions after the fixture assertion cleanup.
- Related compiled Rust selection: 517/20 across 537 functions (library 64/0,
  parser selection 2/0, SQL 129/7, wire 322/13). No previously passing selected
  case becomes red. The expanded SQL selection additionally includes an existing
  FETCH grammar failure; its unchanged expression-token rejection occurs before
  the modified pagination admission path. Other retained failure identities
  are unchanged.
- Complete parser 299/74 and wire core 73/11 retain their previous failure
  identities. These sibling runs preceded the final style-only cleanup.
- Full `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  and `cargo fmt --all -- --check`: passed without suppression. The first full
  gate exposed two runtime style defects. After fixing them, the complete gate
  was rerun and exposed two empty-result fixture assertions. Those assertions
  now print unexpected row payloads. The fixture-only cleanup was verified by
  build, the affected locked test run, full strict Clippy and formatting; no
  third full repository test run is claimed.
- `cntryl-tools validate-tests -f tests/pgwire_extended/protocol/pagination_unary.rs`:
  88/88 compliant. The corresponding `pagination_planning.rs` command is 8/8;
  `tests/support/pagination_unary.rs` also validates. Global test validation:
  6,882/6,882 compliant source tests, not runtime passing totals.
- Inventory refresh/check, touched Rust file limits and whitespace: passed.
  Inventory records 6,009 integration registration instances plus 1,010 library
  tests (7,019 total), distinct from executed/passing totals. No benchmark timing
  is claimed.

Full repository green and PostgreSQL qualification remain incomplete. The native
executable SHA256 remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`, recorded
with the version readback in `target/green-page-native-version.log`.

## 2026-10-16 — SQL-standard FETCH counts and set-query scope

This bundle repairs retained #800 FETCH/OFFSET spelling failures over the
existing integer-bound and CTE-reachability foundation at `6feb79ca`. Complete
#793/#797/#800/#801/#821 acceptance remains open. The batch adds 144 functions:
132 wire, eight stored-row/direct-executor SQL controls and four parser controls.
The first 96-function red run was 26/70. Expanded scope/name controls reproduced
120/8 before the parenthesized set, source namespace and CTE fixes. Final
parameter probes additionally check concrete INT4/INT8 inference and preserve
unknown-plus-unknown ambiguity beneath an outer cast.

`FETCH FIRST/NEXT`, `ROW/ROWS`, omitted count 1 and `ONLY` lower to the existing
limit expression; OFFSET's optional ROW/ROWS suffix uses the standard count
syntax. Parentheses are required around arithmetic, postfix casts or
nonliteral unary expressions in standard counts. Ordinary LIMIT/OFFSET
expression grammar remains available. LIMIT/FETCH share a duplicate-limit guard,
and OFFSET may precede or follow FETCH. Comments, quoted identifiers and clause
ordering have native controls. Valid selected WITH TIES forms reject 0A000;
peer-aware execution remains an open obligation.

Parenthesized set operands use the existing derived-source representation so
local ordering/bounds survive global UNION pagination. Ordinary UNION CTE
bodies remain simple CTEs instead of being mistaken for recursive bodies. Bound
names use the existing alias/own-qualifier source authority before row-variable
rejection: unknown columns report 42703, ambiguous columns 42702 and forbidden
selected integer source variables 42P10. Projection aliases do not enter count scope. The
additive public `CassieError::AmbiguousColumn(String)` preserves native error
identity through SQLSTATE and telemetry (`ambiguous_column`). A concrete integer
sibling selects an arithmetic signature for an otherwise unknown parameter;
unknown-plus-unknown remains ambiguous even inside a result cast.

No new query AST variant, stored Value layout, codec migration, CI/release or
versioning configuration is introduced. Private rebuildable plan epoch 8
invalidates the earlier parsing/binding law. General floating/NUMERIC bound
coercions, complete recursive-CTE behavior, WITH TIES and complete custom Bind
planning remain pending owner obligations, not excluded or completed cells.

Native PostgreSQL 18.6 supplies 240 wire phase/value observations across the 132
wire functions, including text/binary parameter inputs crossed with text/binary
outputs for ten parameter cases. Exact OIDs, typmods, output formats and row
payloads are asserted. Nineteen additional native statements exercise stored
rows, cache-equivalent bounds, unused CTEs, zero/positive DELETE/UPDATE EXISTS
controls and a view definition. Direct execution additionally tests memory
admission and cancellation with zero retained query memory. The native server
version and executable SHA256 are retained in `target/green-fetch-native-version.log`;
the hash remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.

Jev selected lowering to the existing bound AST at 1.0. Count-grammar risk 0.82
prompted sixteen lexical/order/signed-count probes. Adversarial gap 0.58 and set
scope risk 0.45 prompted eight global/local UNION and CTE/EXISTS probes; the
confirmed parser and CTE classification failures were fixed. Subsequent gap
0.35 and parameter risk 0.48 prompted four concrete-sibling/ambiguous-operator
probes, alongside existing pagination sibling controls. Jev is advisory, not
correctness evidence. Ignored `target/green-fetch-*` artifacts retain requests,
native frames, red/green logs, dispositions and validation receipts.

Validation:

- `cargo build --locked --bin cassie`: passed, including the final binary rebuild.
- `cargo test --locked`: completed through the existing catalog/security blocker:
  library 1,014/0, analytics 179/0, backend CI 2/0 and benchmark operations 16/0;
  catalog/security 139/209. All 209 failure identities match the prior batch.
  Later targets compiled but were not executed by that full command. The run
  was not cancelled.
- Final locked targeted runs: parser 4/0, wire 132/0 and SQL 8/0. The four
  additional parameter probes and a validator-driven test-label rename were
  included in final focused execution. No assertion or expected native state
  was weakened; no skip was added.
- Related compiled Rust selection: 683/32 across 715 functions (library 68/0,
  parser selection 2/0, SQL 159/19, wire 454/13). Every previously passing case
  in the retained selection stays green. The retained parameterized FETCH SQL
  failure is fixed. The broader SQL selection includes thirteen previously
  unselected pending cases: explicit DISTINCT/ALL set spellings, VALUES set
  branches, WITH TIES, ORDER BY USING, SELECT ALL and wider numeric expression
  contracts. Their failures do not become qualified or excluded cells.
- Complete parser 299/74 and wire core 73/11 retain exactly their prior failure
  identities.
- Full `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  and `cargo fmt --all -- --check`: passed, including final source checks after
  test-label cleanup. No lint suppression or narrowed Clippy invocation.
- `cntryl-tools validate-tests -f <path>` passes for the eight touched
  `tests/pgwire_extended/protocol/fetch_*.rs` modules (132 functions),
  `tests/sql_queries/expressions/fetch.rs` (eight),
  `src/sql/parser/pagination_fetch_tests.rs` (four), and the two focused support
  modules. Global validation: 7,026/7,026 compliant source tests, not runtime
  passing totals. One ambiguous `_and_` test label was renamed while retaining
  its exact native assertions.
- Inventory refresh/check, touched Rust file limits and whitespace: passed.
  Inventory records 6,149 integration registration instances plus 1,014 library
  tests (7,163 total), distinct from executed/passing totals. No benchmark
  timing is claimed.

Full repository green and PostgreSQL qualification remain incomplete.

## 2026-10-16 — Set quantifiers and VALUES branches

This #800 checkpoint follows the FETCH foundation at `6a648c42`. It adds
201 functions: 180 SQL, 18 wire and three module controls. All 162 initial
SQL cases failed against the prior implementation. Native PostgreSQL 18.6
supplies 180 SQL expectations plus 36 text/binary wire result captures;
`target/green-setspell-native-version.log` records the server and executable
SHA256 `db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`
(Homebrew PostgreSQL 18.6, aarch64 Darwin 25.6). The selected keys are integer/text/NULL, including repeated tuples,
empty inputs, INTERSECT precedence, left association, branch-local bounds,
CTE materialization, comments between operator/quantifier and global ordinals.

Explicit DISTINCT and SELECT ALL now parse; INTERSECT ALL and EXCEPT ALL use
one admitted right-occurrence counter per sorted tuple group. The CTE scalar
boundary reuses the same accounted selector with rich semantic keys. Tests
check cancellation, output-owner release and counter admission at a budget
where UNION ALL output itself still fits. Existing serialized enum variants
are preserved; two additive AST operators and compiled-plan epoch 9 describe
the new behavior. Stored Value formats and storage architecture are unchanged.

Focused probes reproduced two further defects: synthetic branch wrapping lost
the projection needed for global ORDER BY ordinals, and alias lowering used
source carriers after set projection. Both were fixed. Derived set-query column
lists now use the existing relation-alias wrapper; exact native wire cases
exercise VALUES set branches through this path.

Jev selected occurrence counting at 1.0. Gap 0.69 and text/tuple probe weight
0.92 prompted 15 semantic probes plus three CTE tuple controls. A later gap
0.69 and alias probe weight 0.86 prompted readback/re-execution of the 18 exact
native wire cases and the complete scope/alias SQL matrix. Its suggestions are
advisory. Requests, responses, native receipts and raw Rust logs are retained
under ignored `target/green-setspell-*`.

Complete #793/#800/#801 acceptance remains open. Wider common-type coercions,
recursive CTE acceptance, WITH TIES, ORDER BY USING and other row grammar are
pending obligations. This batch does not close a complete owner issue or claim
the repository test suite is green. No benchmark timing is claimed.

Final validation:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 1,017/0, analytics 179/0, backend 2/0,
  benchmark operations 16/0; stops at catalog/security 139/209. All 209
  catalog failure identities match the preceding FETCH checkpoint. Later
  targets compiled but were not executed by this full command.
- Full strict `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed without suppression or narrowed scope.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: passed for all eight new integration
  files and both touched module-test files. Global validation: 7,227/7,227
  compliant source tests; this is not a runtime pass count.
- Selected compiled Rust regression run: library 103/0, parser selection 2/0,
  SQL 346/12 and wire 473/12. All 201 new functions pass. All seven retained
  set-operation failures are fixed. The unrelated Boolean-aggregate
  cancellation test passed this run; no stable remediation is claimed for it.
- Complete parser target: 299/74 and complete core wire: 73/11, both with
  unchanged failure identities. No prior passing selected case becomes red.
- Inventory refresh/check, Rust file limits and whitespace: passed. Inventory
  contains 6,347 integration registrations plus 1,017 library functions.

A preliminary gate overlapped an unfinished signature edit and failed to
compile; after correction, the complete ordered gate above was rerun. Initial
focused green attempts also exposed the ordinal, exported-name and derived
alias defects before their fixes. No failures were skipped or assertions
weakened, and no full run was cancelled.

## 2026-10-16 — Typed VALUES scalar identity and character ownership

This #800 checkpoint follows set quantifiers at `cf85b849`. It adds 258
functions: 210 wire, 42 SQL text-conversion witnesses and six SQL resource/cache
controls. The first 168-function wire run reproduced 24 passing/144 failing
cases before runtime changes. Native PostgreSQL 18.6 provides 448 wire
input/output-format observations plus 42 text-conversion statements. Version
and executable SHA256 are retained in `target/green-valuescasts-native-version.log`:
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`,
Homebrew aarch64 Darwin 25.6.

Selected existing DATE/TIME/TIMESTAMP/UUID/BYTEA casts are now admitted in
VALUES, including NULL/unknown common-column inputs, standalone/derived/CTE/set
contexts and inferred/supplied input OIDs. Inferred parameters cross text/binary
inputs and outputs. Assertions check native names, OIDs, sizes, typmods, exact
row bytes and five prepared-statement lifecycle phases. Common-column coercion
retains source identity for character and temporal text conversion.

SQL CHAR defaults to width 1; internal unbounded bpchar advertises typmod -1.
Explicit CHAR casts pad/truncate by Unicode character count. Character-to-text
coercion and comparison signature selection preserve PostgreSQL trailing-blank
laws. Padded output values remain intact while declared character keys ignore
blanks for selected comparisons and DISTINCT. Projection and CTE descriptor
handoffs retain character identity with admitted backing and owner lifetimes.
Compiled plans rebuild at epoch 10; AST variants, public API surface and stored
Value layouts are unchanged.

Jev selected reuse of the existing cast pipeline at 1.0. Gap 0.70 and character
probe weight 0.95 prompted 16 typmod/coercion controls. Eight semantic probes
then reproduced 1/7 after adding padding; comparison and DISTINCT defects were
fixed, producing 8/0. Follow-up gap 0.65, memory weight 0.47 and CTE weight 0.35
prompted two more resource tests and four exact native CTE controls. CTE
DISTINCT first returned 2 instead of native 1; the remaining ARRAY-only metadata
gate was corrected to retain CHAR as well, producing 4/0. Constant and parameter
CHAR(100000) under a 16 KiB budget reject without retaining owners; a retry of
the same parameter plan with a larger budget returns the full padded value and
releases its owners on drop. Constant folding and projection admit character
expansion before allocation. Final focused advisory judgments: CTE risk
addressed 0.76, selected resource coverage bounded 1.0; neither establishes full
#849 acceptance. Requests, responses, native receipts and logs are under ignored
`target/green-valuescasts-*`.

Native locale sorting of symbols differs from Cassie's current byte ordering.
The exact symbol/letter probes were retained in the native receipts, and the
identity cases were recaptured with independent integer ordering or ASCII
prefixes while preserving Unicode input/value assertions. This is an open
collation obligation under [#796](https://github.com/cntryl/cassie/issues/796),
whose acceptance explicitly includes libc/ICU comparison, ordering and keys.
No collation completion is claimed. Broader type/input errors, coercions,
functions/subqueries in VALUES and full #793/#800/#849 acceptance remain open.
No benchmark timing is claimed.

Final ordered validation:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 1,017/0, analytics 179/0, backend 2/0,
  benchmark operations 16/0; stops at catalog/security 139/209. All 209
  failure identities match `cf85b849`. Later targets compiled but were not
  executed by this command.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed without suppression or narrowing.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: all eleven new wire files, both SQL
  files and the shared support file pass. Global validator: 7,485/7,485 compliant
  source tests, not runtime pass totals.
- Final selected compiled Rust run: library 110/0, parser selection 2/0,
  SQL 397/17 and wire 683/12. All 258 new functions pass. Retained timestamp
  VALUES text identity, mixed character coercion and unconstrained character
  parameter tests are fixed. Seven additional existing cast tests were selected:
  five user-defined/composite cast prerequisites remain absent, explicit
  VARCHAR truncation remains pending, and the older cast-error evidence fixture
  still expects a generic message instead of the native integer error identity.
  Their failure triggers are outside the implemented slice; no assertion was
  weakened. These are newly selected cases, not newly failing baseline cases.
- Boolean-aggregate cancellation returned 54000 instead of 57014 in this run.
  This differs from the preceding batch's passing observation but exactly
  repeats the earlier FETCH receipt at
  `target/green-fetch-regression-pgwire_extended.log:516`. Its stability was
  already unqualified; #805/#849 cancellation/resource acceptance remains open.
- Complete parser 299/74 and core wire 73/11: unchanged failure identities.
- Inventory refresh/check, Rust file limits and whitespace: passed. The
  integration inventory adds exactly 258 identities, removes none and changes
  no existing test record. It contains 6,605 registrations plus 1,017 library
  functions. Large textual inventory diffs come from inserting generated records.

The preliminary gate reproduced the catalog failures and one pedantic numeric
literal finding in a test. After that correction and the confirmed CTE metadata
fix, the full ordered gate was rerun. Final focused/source-family checks use the
compiled final binaries. No full run was cancelled; no failures were skipped.

### Explicit VARCHAR typmods checkpoint (2026-10-16)

Following `a5be39d5`, the related cast bundle adds 200 functions: 102 wire and
98 SQL/control tests. PostgreSQL 18.6 supplies 180 query observations in both
output formats, 48 crossed parameter-format observations and six assignment
witnesses. The native executable SHA256 remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Exact row bytes, names, OIDs, sizes, typmods, NULLs and statement lifecycle are
asserted across scalar, VALUES, derived, CTE and ordered UNION ALL contexts.
Inputs include ASCII, multibyte/combining characters, spaces, CHAR, integers,
Boolean, FLOAT8, TIME and TIMESTAMP; supplied TEXT/VARCHAR/bpchar parameters
cross both input and output formats.

Explicit VARCHAR casts truncate the source-aware text String at a valid UTF-8
character boundary. Short input is not padded; NULL remains NULL. The existing
assignment/function coercion path stays bounded. Compiled-plan epoch 11
invalidates older results; public AST/API, stored Value layout and CI/release
configuration are unchanged.

The preliminary broad `varchar_` filters reproduced 80 wire failures (78 new)
and 76 SQL failures. Two SQL failures initially used an incorrect expected
assignment message; three used unsupported casts inside INSERT VALUES. The
controls now assert the existing strict assignment message and exercise explicit
casts through supported INSERT SELECT, recaptured against native PostgreSQL.
The first green attempt exposed 17 set fixtures asserting row sequence without
ORDER BY. Explicit ordering was added and native expectations recaptured;
value/descriptor assertions were retained.

Jev selected explicit-only interception at 1.0. Gap 0.53 prompted assignment
controls; cache rebinding, NULL and large-width/short-input controls also pass.
Final gap 0.51 prompted four focused padded-CHAR/bpchar probes. All four pass
against native receipts; final advisory disposition is `probed` at 1.0.
VARCHAR(100000) with a short parameter fits a 16 KiB budget and releases its
result owners. Full #849 resource/cancellation qualification remains open.

Ordered validation and final checks:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 1,017/0, analytics 179/0, backend 2/0,
  benchmark operations 16/0; stops at catalog/security 139/209. All 209 failure
  identities match `a5be39d5`; later targets compile but are not executed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  final pass without suppression or narrowing. The first run found test-source
  Unicode/empty-string/empty-assertion style defects. Decomposed characters now
  use explicit Unicode escapes with identical runtime bytes. After these
  test-only fixes, build, test-target compilation, full Clippy and formatting
  passed; final compiled runs below verify the unchanged expectations.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: all twelve new files pass.
  Global validator: 7,685/7,685 compliant source tests, not runtime pass totals.
- Final compiled checks: library 110/0, parser selection 2/0, SQL 496/16,
  wire 786/14. All 200 new functions pass. Retained SQL type-boundary and
  core-wire explicit-character truncation tests are fixed.
- Two newly selected wire failures (unbounded VARCHAR arrays across restart and
  JSON serialization RETURNING VARCHAR) already failed in the pre-change red
  run. No previously selected failure was introduced. The known Boolean
  aggregate cancellation failure remains unqualified.
- Complete parser 299/74 retains its exact failure identities. Core wire is
  74/10, down from 73/11, with no new failures.
- Inventory refresh/check and whitespace pass. Exactly 200 identities are added;
  none are removed and no existing test record changes. The inventory contains
  6,805 integration registrations plus 1,017 library functions.
- All touched Rust files satisfy their size limits. The audit also retains two
  untouched baseline exceptions: src/config.rs (1,034) and
  src/midge/adapter/verification.rs (1,002). No benchmark timing is claimed.

Native assignment overflow reports 22001; Cassie's existing assignment class
remains 22000. Full assignment error/coercion parity, general INSERT VALUES
expressions, user-defined casts and complete #793/#800/#911 acceptance remain
pending. This checkpoint does not close owner issues or establish repository
or hosted green. Receipts and logs are under ignored `target/green-varchar-*`.

### Integer/BYTEA casts and bound constants checkpoint (2026-10-16)

Following `b181036f`, the related #816/#793/#800 bundle adds 416 functions:
225 wire, 187 SQL and four library admission controls. All pass against final
compiled binaries. Native PostgreSQL 18.6 receipts retain both output formats,
crossed supplied parameter formats, constant/parameter Bind errors, statement
recovery and exact values, NULLs, names, OIDs, sizes, typmods and row bytes.
The executable SHA256 remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.

INT2/INT4/INT8-to-BYTEA uses the declared source width and big-endian two's
complement. Reverse conversions zero-pad shorter inputs, interpret full-width
values as signed, return zero for empty input and reject excess bytes even
when they are leading zeroes. The additive public IntegerByteaError exposes
width/byte_count; CassieError::IntegerBytea reports native 22003 range messages.
Existing Cast source metadata and canonical BYTEA storage are reused; epoch 12
invalidates older compiled plans. No stored Value/AST layout or migration,
CI/release/version configuration or second storage abstraction changes.

The initial selected run reproduced 259 wire and 194 SQL failures, including
397 of the initial 404 new functions; seven new controls already passed. The
first green attempt passed all 187 new SQL tests but exposed 21 wire gaps:
native range-message identity, CTE propagation and parameter-dependent planning
phases. Eight additional wire scope/laziness probes and four library controls
were added. No native phase, payload or error assertion was relaxed.

Bind moves decoded parameter ownership through its blocking validation task
and back into the portal without cloning the vector or changing cached plans.
Execute uses the same bound-value law. The existing lazy semantic evaluator
admits only reached primitive parameter copies, so dead CASE/COALESCE/AND/OR
branches stay dead. Pure parameter-free one-row VALUES CTEs with identity
output reuse the existing admitted source inliner. Canonical exported-name
lookup is admitted before mutation, preserving ordinary and quoted aliases;
other CTE shapes retain their prior path.

Jev selected typed-width conversion at 1.0; gap 0.80 prompted four mixed-width
VALUES probes. Later gap 0.46 prompted native/Rust good → Bind error → good
recovery checks on cached CASE/COALESCE statements across both input/output
formats. All pass. Large dead parameters (200 KB) fit a 16 KiB budget without
being copied or charged, while reached copies reject before allocation and
release their owners. A one-byte budget rejects fixed BYTEA constant output
before allocation with peak ownership zero. Source/result clone, cancellation,
small-budget retry and changed/NULL parameter controls also pass. Final advisory
dispositions: cached risk probed 1.0, selected admission bounded 1.0; full #849
qualification remains open.

The user interruption left unusable incremental linker artifacts. The final
build/test/Clippy lane ran with CARGO_INCREMENTAL=0, without deleting source or
weakening checks. Preflight Clippy found array-chunk syntax, helper placement
and an oversized test function; these were corrected by standard array chunks,
function placement and focused shared fixtures. The final ordered gate ran
after all production fixes:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: library 1,021/0, analytics 179/0, backend 2/0,
  benchmark operations 16/0; stops at catalog/security 139/209. All 209 failure
  identities match `b181036f`; later targets compile but do not execute here.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed without suppression or narrowing.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: all 25 touched test/support/root files
  pass. Global validator: 8,101/8,101 compliant source tests, not runtime totals.
- Fresh Cargo artifact metadata identifies the final test binaries. Expanded
  compiled SQL is 691/30, wire 1,011/56. All 412 new integration functions and
  four new library controls pass. No previously selected regression occurs.
  The additional 14 SQL and 42 wire failures all already failed in the
  pre-change red run. They are newly selected existing obligations, including
  BYTEA functions/input formats, BIT/VARBIT and related aggregate/hash behavior.
- Complete parser 299/74 and core wire 74/10 retain their exact failure identities.
- Inventory refresh/check, whitespace and touched Rust file limits pass. Exactly
  412 integration identities are added; none are removed and no existing test
  record changes. There are 7,217 integration registrations plus 1,021 library
  functions. Untouched size exceptions remain src/config.rs (1,034) and
  src/midge/adapter/verification.rs (1,002). No benchmark timing is claimed.

Full type/input/signature, function, CTE, resource/cancellation/restart and
owner qualification remains open under #793/#800/#803/#816/#849. JSON/Vector
parameters remain variable in this bound constant path. BYTEA escape/session
output, wider casts/functions, BIT/network/UUID/hash/LSN and complex or
parameterized CTE planning are not completed by this checkpoint. Known Boolean
aggregate cancellation remains unqualified. Full repository/hosted green is
not established. Native receipts, advisory requests/responses and logs are
under ignored `target/green-byteacasts-*`.

### BYTEA hex/escape input checkpoint (2026-10-16)

Building on `f7ccbd89`, SQL BYTEA casts and scalar text Bind now share borrowed
input validation. Ordinary UTF-8 bytes, octal escapes 000–377, doubled
backslashes and hex input canonicalize to the existing lower-case hex carrier.
Hex whitespace is accepted between complete pairs; malformed escapes report
22P02 and malformed hex reports 22023 with native invalid-digit/odd precedence.
The additive public ByteaInputError/CassieError variants retain bounded typed
diagnostics; selected ASCII messages match PostgreSQL. Compiled plans rebuild
at epoch 13 without a persistent format, AST, storage or CI/release change.

This bundle adds 224 functions: 167 wire, 54 SQL and three library controls.
The initial 200 integration functions produced 184 failures and 16 passes.
Native PostgreSQL 18.6 uses the unchanged pinned executable SHA256
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Quoted invalid BYTEA fails at Parse even in dead CASE arms; explicit TEXT
constants and parameters fail at Bind, and stored TEXT fails at execution.
Both parameter input formats and output formats retain OID 17, native names,
sizes, typmods and exact payload bytes. Failed Bind preserves the statement
for a valid rebind. Stored-column assignment/read, cached good/error/NULL/good
values, result-clone ownership and pre-cancellation probes accompany the matrix.

The first memory probe exposed structural charging of a dead 100 KB literal
conversion. Reached-cast admission in the existing lazy semantic evaluator
corrected it: live expansion rejects before allocation under a small budget,
dead expansion fits that budget, and invalid-input scratch releases. Unbound
constant checks now also use the controlled semantic path; the unused private
uncontrolled wrapper was removed. Existing generic fallback behavior remains.
Preflight strict Clippy also caught item placement, corrected without lint
suppression.

Jev's design-gap question returned 0.78 and selected precedence probes: 12
native/Rust cases cover invalid digit versus odd input, whitespace within pairs,
octal followed by digits and UTF-8 byte interpretation. Review gap 0.50 selected
carrier coverage: all 256 octal byte values pass native and Rust literal/OID 17/
TEXT parameter paths across output formats, including non-UTF8 binary bytes.
Final advisory dispositions are carrier probed 1.0 and selected resource scope
1.0. Per-portal decoded parameter retention and complete transport/resource
qualification remain open under #849; no complete resource claim is made.

The ordered gate ran after all production fixes with CARGO_INCREMENTAL=0:

- `cargo build --locked --bin cassie`: passed (26.36 seconds).
- `cargo test --locked`: library 1,024/0, analytics 179/0, backend 2/0 and
  benchmark operations 16/0. Stops at catalog/security 139/209 after 436.45
  seconds; all 209 failure identities match `f7ccbd89`. Later targets compile
  but do not execute in this command.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed without suppression or narrowing.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: all 20 touched test/support/root
  files pass the final validator run. Global validator: 8,325/8,325 compliant source tests.
- Fresh Cargo artifact metadata identifies final binaries. All 224 new
  functions pass. Expanded SQL is 746/29 and wire 1,179/55, with no newly
  failing previously selected tests. Retained SQL BYTEA input-format and wire
  malformed-text Bind SQLSTATE tests now pass; the other failures remain open.
- Complete parser remains 299/74 with unchanged failure identities. The first
  core-wire run exposed one stale assertion expecting 08P01 for odd BYTEA hex.
  An identical native prepared query/frame sequence confirms Bind 22023 and
  `invalid hexadecimal data: odd number of digits`. The assertion now pins
  that state and exact message; unrelated legacy UUID expectations are retained.
  The final ordered gate repeats after this correction: build passes in 0.13
  seconds; full locked tests again reach the same 139/209 catalog outcome in
  270.75 seconds after 1,024/0 library and the same clean earlier suites.
  Full strict Clippy and formatting pass. Complete core wire returns 74/10
  with the same remaining failure identities as `f7ccbd89`.
- Inventory refresh/check and whitespace pass. Exactly 221 integration IDs
  are added and none removed. Two existing inventory line ranges update for
  the strengthened core BYTEA assertion and its following test. Inventory contains
  7,438 integration registrations plus 1,024 library functions; these are
  instance counts, not passing-test totals. All touched Rust files satisfy
  their limits. Untouched exceptions remain src/config.rs (1,034) and
  src/midge/adapter/verification.rs (1,002). No benchmark timing is claimed.

Complete #793/#803/#816/#849 acceptance remains open. encode/decode and wider
binary functions, session BYTEA output selection, non-ASCII diagnostic-byte
fidelity, wider casts/types, complex CTE planning and full resource/restart
qualification are pending obligations. Full repository and hosted green are
not established. Native receipts, advisory requests/responses and complete logs
are under ignored `target/green-byteainput-*`.

### Catalog/security read-model checkpoint (2026-10-16)

The fresh `b0d2b65e` catalog/security run reproduced 139 passing and 209 failing
functions. Of those failures, 179 were grouped under catalog/type information;
30 covered wider session, transaction and administration behavior. Current
#817/#793/#831/#841 requirements and the existing Midge catalog source defined
this read-model increment; no persistent metadata, type or layout changed.

The new experimental registry exposes PostgreSQL 18.6 column-name shapes for
26 object families absent from current metadata. Empty rows reflect the absence
of those objects, without implementing their DDL. Dependency, rewrite,
statistics, operator-family and built-in text-search catalogs are omitted
because they require actual semantic rows. Table/view/sequence discovery reads
real database-scoped Catalog metadata. Ordinary and SERIAL/BIGSERIAL columns
report NO/NULL/NEVER identity/generation flags. Compiled plans rebuild at epoch
14. Ownership is not recorded, so new owner fields are NULL; definitions expose
stored SQL rather than native pretty printing. Native catalog-domain/OID/opaque
codec ABI remains explicitly unqualified under #793/#817.

Adds 68 Rust functions: 42 catalog/security and 26 wire checks. The first 32
registry witnesses and seven object-lifecycle witnesses all failed before
implementation. The initial implementation fixed 30 original failures. Final
oracle review caught an incorrect new-test assumption: native hasindexes stays
true after DROP INDEX until maintenance, while the current read model clears
it immediately. No false-after-drop native assertion is retained. Selected flag
probes instead pin false-before-create and true-after-create, which match native
observations. The historical flag cell remains an explicit #817 follow-up that
needs its persistent-state/lifecycle design; no ad hoc metadata field is added.
A separate native autocommit/temp-table probe records false → true → true
through table/index/drop, then false after explicit VACUUM. The precise gap and
reproduction remain tracked under #817. All original expectations are unchanged.

Jev triage selected read models at 0.97, with gap 0.79 and an object-scope
constraint at 1.0. Excluding catalogs requiring actual implemented-object rows
and adding database/lifecycle/restart controls addresses that constraint.
Adversarial gap 0.52 selected sequence state: standalone nextval() and CREATE
SEQUENCE AS/options are absent and remain #841 obligations with their retained
advanced tests still red. Supported SERIAL/BIGSERIAL omitted-field INSERT
probes match native unused NULL → 1 → 2 state and declared integer/bigint bounds.
An additional same-name/different-database index probe passes. Final advisory
dispositions are selected probes complete 1.0 and staged support 1.0; neither
closes a full owner or proves correctness independently. A subsequent advisory
review includes the independently decoded historical-index mismatch and chooses
the explicitly staged checkpoint at 1.0; #817 remains open.

PostgreSQL 18.6 supplies native pg_attribute column names/types and selected
row/lifecycle observations; its executable SHA256 remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Native DDL probes use rollback-only oracle transactions, closing named
portals between format binds; the separate VACUUM probe uses an autocommit
temporary table. Current Cassie wire probes check statement/portal
names, formats and zero DataRows across text/binary output. They do not equate
existing logical carriers with native catalog-domain descriptors.

The ordered gate ran after all production fixes with CARGO_INCREMENTAL=0:

- `cargo build --locked --bin cassie`: passed (35.70 seconds).
- `cargo test --locked`: library 1,024/0, analytics 179/0, backend 2/0 and
  benchmark operations 16/0. Catalog/security is 211/179: exactly 30 original
  failures fixed, none added. The command stops there after 436.59 seconds;
  later targets compile but do not execute in this command.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed without narrowing or suppression. Preflight empty-collection assertions
  were strengthened to show values on failure.
- `cargo fmt --all -- --check`: passed.
- The validator flagged `_and_` in one new lifecycle test name. The name alone
  was corrected; assertions and production code are unchanged. Fresh target
  compilation, full strict Clippy, formatting and all eight touched-file
  validators then pass. Global validator: 8,393/8,393 compliant source tests.
- All 68 new functions pass against fresh binaries: 42 catalog/security
  functions rechecked after both test corrections, plus all 26 wire exports.
  The final full catalog binary rerun is again 211/179 (95.85 seconds), fixing
  the same 30 original failures without adding failures.
  Complete parser remains 299/74 and core wire 74/10, with unchanged failure
  identities. No new catalog/security failure identity occurs.
- Inventory refresh/check and whitespace pass: exactly 68 integration IDs
  added, none removed and no existing record changed. There are 7,506 integration
  registrations plus 1,024 library functions; these are instance counts, not
  passing-test totals. All touched Rust files meet their limits. Untouched
  exceptions remain src/config.rs (1,034) and src/midge/adapter/verification.rs
  (1,002). No benchmark timing is claimed.

The remaining 179 failures require native catalog/type identities and OID/reg*
lifecycle, actual dependency/system catalog rows, definition/privilege functions,
ownership, transaction/DDL/sequence foundations and wider administration work.
Complete #793/#817/#831/#841/#849 and repository/hosted green remain open. The
legacy CREATE SEQUENCE integer-default difference also remains #841 work; this
read-model checkpoint reports the stored type rather than inventing bigint
metadata. Native receipts, advisory questions/actions and logs are under ignored
`target/catalog209-*`.

### Catalog size and encoding scalar checkpoint (2026-10-17)

The user's fresh `c06e9a5b` run reproduced 211 passing / 179 failing
catalog/security functions. This coherent #817 increment selects primitive
size parsing, bigint formatting and encoding-name lookup. #793's merged common
records permit the existing primitive paths; NAME ABI, NUMERIC runtime,
physical storage sizes and wider session/encoding work remain staged obligations.
No persistent type, metadata, storage layout, dependency or CI configuration
changes. Compiled plans rebuild at epoch 15.

Adds 204 Rust functions: 198 wire and six catalog/security controls. The first
147 oracle-backed wire witnesses all failed on the unchanged runtime. The first
implementation returned 162/35 across the expanded 197-function wire matrix.
That exposed NAME coercion, planning-error timing and numeric/unit precedence;
the selected helpers now match those native observations. Error syntax/range
categories use the additive SizeInputError/CassieError::SizeInput API and are
recognized by constant planning without string-based size-error classification.
A fixture's private descriptor access was replaced with its public typed error;
the cancellation witness normalizes the executor's existing general error through
the public CassieError conversion, retaining the canonical cancellation check.
No original test source or expectation changes.

Independent PostgreSQL 18.6 observations cover all 81 encoding aliases,
normalization/truncation, decimal/exponent/unit syntax, exact near-half rounding,
bigint limits, selected error SQLSTATE/message and Parse/Bind phases, and
parameter/result formats. Its executable SHA256 remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
The function's actual input is NAME, not TEXT: native untyped parameter metadata
is OID 19 (`000100000013`). Current literal/stored TEXT/explicit TEXT parameter
paths apply NAME's 63-byte UTF-8 truncation before lookup. Bare unknown NAME
parameter inference rejects with 0A000 pending its real ABI, rather than
advertising a substitute TEXT identity. Lookup is stable, not immutable; broader
locale/encoding behavior remains #821. Size tests do not approximate the pending
NUMERIC formatting overload through floating point.

Jev triage selected the scalar bundle at 1.0 with gap 0.82. Source review returned
gap 0.90 and selected numeric/unit precedence at 0.91. Fourteen focused native
and Rust witnesses establish that NUMERIC input bounds precede unit validation,
valid huge NUMERIC plus an invalid unit reports 22023, final int8 overflow reports
22003, zero may carry exponent 1073741823 but not 1073741824, and decimal inputs
around halfway boundaries retain exact rounding. Those findings drove the
specific bounds/precedence corrections. A subsequent NAME-boundary disposition
selects staged rejection at 0.97; final selected-precision and staged-claim
probabilities are 0.99 and 0.96. These are advisory judgments, not verification.

The final ordered gate ran with CARGO_INCREMENTAL=0:

- `cargo build --locked --bin cassie`: passed (20.44 seconds).
- `cargo test --locked`: library 1,024/0, analytics 179/0, backend 2/0 and
  benchmark operations 16/0. Stops at catalog/security **220/176** after 524.92
  seconds. Exactly three original failures are fixed: encoding-name mapping,
  size-string parsing and integer size formatting. None of the original passing
  tests becomes failing. All six new catalog/security functions pass. Later
  targets compile but do not execute in this command.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed (36.63 seconds) without narrowing or suppression. Preflight midpoint,
  clamp and function-size findings were fixed with the idioms and focused
  signature helpers; no lint level changes.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: all 13 touched test/root files pass.
  Global validation: 8,597/8,597 compliant source tests.
- `cargo test --locked --no-run --message-format=json` identifies fresh final
  binaries. The wire binary's `catalog_scalar_` filter passes all 198 functions
  (21.61 seconds). Complete parser remains 299/74 and core wire 74/10; both
  preserve their exact failure identities from `c06e9a5b`.
- Inventory refresh/check adds 204 IDs, removes none and changes no existing
  record. There are 7,710 integration registrations plus 1,024 library functions;
  these are instance counts, not passing totals. Whitespace and all touched Rust
  file limits pass. Untouched exceptions remain src/config.rs (1,034) and
  src/midge/adapter/verification.rs (1,002). No benchmark timing is claimed.

Stored-row projection/filtering, good/error/NULL/good cached-plan inputs,
last-result-owner release and already-cancelled execution accompany the wire
matrix. The cancellation witness does not qualify interruption inside a long
parser loop; full resource/restart acceptance remains #849. Native NAME parameter
ABI, NUMERIC formatting, physical size APIs, wider locale/encoding, historical
hasindexes behavior and the remaining catalog/identity/ownership/administration
obligations remain open under #793/#817/#821/#849 and their linked owners.
Full repository and hosted green are not established. Native observations,
source receipts, advisory requests/responses and logs are under ignored
`target/catalog-next-*`.

### Runtime capability and information checkpoint (2026-10-17)

This #817 increment follows d3ac95b2 (catalog/security 220/176). The selected
bundle uses existing BOOL/TEXT carriers: recovery/JIT/NUMA capabilities,
actual Rust Unicode version, compression of materialized SQL values, and the
empty three-TEXT-column loaded-module FROM relation. Native scalar RECORD
projection remains explicitly unsupported under #793/#817. No stored format,
layout, dependency or CI/release/versioning configuration changes occur.
Compiled plans rebuild at epoch 16.

The pinned PostgreSQL 18.6 executable retains SHA256
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Native probes establish selected descriptors, empty module rows, compression
NULL, explicit primitive parameters and native 42883/42P18 signature failures.
Capability values are Cassie's actual profile, independent of the oracle's JIT
build or Unicode tables. `unicode_version()` is checked against Rust's actual
`std::char::UNICODE_VERSION`; only this helper is immutable. Compression says
nothing about Midge's block codecs and still evaluates its argument.

All 45 initial wire witnesses failed before implementation. The first expanded
pass was 43/16: fifteen generated profile expectations used the wrong BOOL text
encoding (`false` instead of native `f`); those fixtures were corrected. The
remaining failure exposed duplicate qualified lookup fields in wildcard CTE
schema inference. Table-function schema inference now respects qualification
without treating lookup aliases as additional outputs. Further native alias
probes exposed rejected loaded-module aliases, repaired through the existing
relation-alias parser. The fixture also now distinguishes ParseFlush from a
Sync-completed cycle; it does not require ReadyForQuery before Sync.

Jev selected the capability bundle at 1.0, with initial probe gap 0.83. Source
review returned scalar-RECORD overclaim probability 0.21, argument-evaluation
gap 0.70 and qualified-shape gap 0.66. The latter two triggered permanent
invalid stored-argument-cast and qualified/aliased CTE/FROM probes. Native
invalid-cast evidence reports 22P02; Cassie must propagate the argument failure.
These model judgments guide probes and do not prove correctness. Complete
system/catalog ABI, owner acceptance and #849 resource qualification stay open.

Validation results are recorded below after execution. All Cargo commands use
`CARGO_INCREMENTAL=0`; original test expectations and lint levels are retained.

Ordered final validation:

- `cargo build --locked --bin cassie`: passed (18.47 seconds).
- `cargo test --locked`: library 1,024/0, analytics 179/0, backend 2/0 and
  benchmark operations 16/0 passed. The command stops at catalog/security
  **229/173** after 408.92 seconds. Exactly three original failures are fixed:
  runtime capabilities, single-node recovery and uncompressed text property.
  No newly failing original test; the full repository remains red.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed. Preflight redundant-binding and fixture Arc type findings were fixed.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f <path>`: all nine touched Rust test files
  passed; global `cntryl-tools validate-tests`: 8,664/8,664 compliant source tests.
- `target/test-inventory-tools312/bin/python tools/test_inventory.py --check`
  and `git diff --check`: passed. Inventory adds exactly 67 identities, removes
  none and changes no existing record. All 23 touched Rust files fit their
  source/support or integration-root line limits.

The coherent new batch passes 61 wire and six catalog tests. It includes native
text/binary descriptors and payloads, typed primitive parameter format crosses,
arity/unresolved signatures, Sync recovery, actual Unicode profile, stored
compression, propagated cast errors, empty-set COUNT, left-row preservation and
pre-cancelled discovery with zero retained query memory. Pre-cancellation does
not qualify interruption during long-running work; #849 remains open.

Fresh `cargo test --locked --no-run --message-format=json` artifact discovery
passed. Final executable reruns pass all 61 wire checks. Complete parser remains
299/74 and core wire remains 74/10, with exactly the preceding failure identity
sets. All seven `domain_models` graph-function correctness checks pass, covering
shared table-function inference callers. Final Jev review selects finite staged
support at 1.0 and addressed probe action at probability 0.79. No owner is closed
and no full repository or hosted green is claimed.

### Midge 0.4.1 fresh-store checkpoint (2026-10-17)

The user upgraded Cargo dependencies and explicitly selected fresh stores.
Published `cntryl-midge` 0.4.1 requires FORMAT 8; pre-0.4 stores require logical
export/import into a new store. This change does not migrate, remove or open an
existing deployment's store. It retains the user-generated lockfile refresh
and documents the fresh-store boundary in README and storage qualification.
Cassie's own logical layout and CI/release/version configuration are unchanged.

The red `outcome_tests::` unit returned `Some(0ns)` for
`OutcomeUnknown("write conflict while confirming publication")`; the required
result is no automatic retry. Mapping now uses typed `WriteConflict` and keeps
unknown outcomes terminal with reconciliation guidance. A corruption message
containing conflict text also remains terminal. Native storage backpressure,
fencing and bounded conflict retries retain their existing behavior.

Jev triage selected compatibility assessment at 1.0 and identified an unknown
outcome regression gap of 0.87. The permanent retry regression confirmed it.
Implementation review selected fresh-only scope and a post-recovery write
probe at 1.0, with 0.77 probability that the typed correction eliminates the
selected retry path. The added witness commits after recovery, reopens again
and verifies both commits. These judgments do not qualify a deployment or prove
all recovery behavior. Cloud-provider and old-store migration acceptance remain
separate and open.

Eight new tests cover retry refusal, terminal error detail/guidance, terminal
corruption despite conflict text, fresh FORMAT 8 creation, old-marker rejection,
typed row/view/index metadata recovery and writes after restart. The preserved
47fce044 storage executable establishes 154 passed/six failed/one ignored before
this change. Its existing failures cover cross-type operator families, range
indexes, TIMESTAMPTZ and text-search mapping changes. Ordered validation and
fresh executable results follow below. All Cargo uses `CARGO_INCREMENTAL=0`.

Final executed evidence:

- `cargo build --locked --bin cassie`: passed (24.91 seconds).
- `cargo test --locked`: library **1,025/0**, analytics **179/0**, backend
  **2/0**, benchmark operations **16/0**. Stops at catalog/security **229/173**
  after 584.42 seconds; its exact failure identities match 47fce044.
- Fresh artifact discovery with
  `cargo test --locked --no-run --message-format=json`: passed. The final
  storage executable reports **161 passed/six failed/one ignored**; its six
  failure identities exactly match the preserved pre-upgrade binary. All seven
  new integration tests pass; the eighth new test is the library retry unit.
- Complete parser **299/74** and core wire **74/10** preserve their exact failure
  sets. The selected runtime wire matrix remains **61/0**.
- Full `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  and `cargo fmt --all -- --check`: passed, including the final renamed test.
- The validator initially flagged `_and_` in one new test name. The test was
  renamed without changing assertions. All three touched integration validators
  plus `cntryl-tools validate-tests -f src/midge/adapter/documents/commit.rs`
  now pass. Global validator: **8,672/8,672** compliant source tests.
- Inventory refresh/check adds seven integration identities, removes none and
  changes no existing identity. The added unit brings the new-test total to
  eight. Whitespace and touched Rust line limits pass.

Final Jev review selects fresh-store scope at 1.0 and the selected retry
regression as verified at probability 0.99. No deployment migration or provider
qualification is claimed. Existing 173 catalog failures and six storage-suite
failures remain; full repository and hosted green are not established.

### Option-table and TEXT[] input checkpoint (2026-10-17)

This #817/#796 increment follows 0c53a092 (catalog/security 229/173). It
selects `FROM pg_options_to_table` over one-dimensional logical TEXT arrays,
with two native TEXT output columns. The first `=` splits each option; absent
`=` yields NULL, and empty names/values, whitespace, duplicates and literal
contents are retained. NULL/empty arrays produce no rows, while NULL elements
raise typed 22004 without partial rows. Selected malformed array text reports
22P02 at Execute. DISTINCT/in-call ORDER BY report native 42809 at Parse.

The selected prerequisite adds explicit one-dimensional string/NULL TEXT[]
constructors and TEXT-to-array casts over the existing logical carrier. Table
execution consumes the typed scalar array representation; it does not alter
global JSON materialization. Existing table-function binding/schema code moves
from select.rs into a focused module. An aliased lateral wrapper now reaches
normal outer-column lowering; implicit lateral option calls use the same path.
Scalar RECORD projection remains explicitly unsupported. Native array OID 1009
is absent: direct native array parameters reject with 0A000, while explicitly
TEXT-typed parameters cast into arrays preserve OID 25. The private logical
array identity is unchanged. No persistent format, dependency or CI changes;
compiled plans rebuild at epoch 17.

Native PostgreSQL 18.6 retains executable SHA256
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
All 48 initial wire witnesses failed before implementation. Expanded probes
exposed the missing constructor/cast path and the scalar-JSON materialization
boundary. Subsequent native inputs preserve quotes, backslashes, commas,
Unicode and quoted NULL. The oracle database uses en_US.UTF-8 collation, whose
mixed-case/Unicode ordering differs from Cassie's current ordering. The 64 new
constant/escaping queries were recaptured without locale ORDER BY to qualify
native array emission order; complete ordered row payloads remain asserted.
Original repository tests and assertions were not changed. Collation remains
open under #796/#793/#817. Six TEXT parameter probes retain selected lowercase
ORDER BY checks. This checkpoint does not establish locale parity.

Jev selected the option bundle at probability 0.94 with probe gap 0.70. Source
inspection corrected the initial native-array-parameter assumption before any
support claim. Review selected escaping probes at 0.93 and a cast-scratch gap
at 0.30. The constant CHAR-budget probe already passed. The stored-column probe
first exposed the alias binding omission, then failed its memory assertion
(7/1 catalog iteration). Argument execution now admits the existing expanding
cast scratch estimate before evaluation; that unchanged regression passes.
All 95 new tests pass in the focused rerun: 86 wire and nine catalog. They
include good/error/good prepared reuse, native errors and metadata, stored
logical arrays, explicit/implicit lateral reads, empty outer joins,
pre-cancellation, budget denial, last-result-owner release and error release.
Stored arrays are seeded through the public ingest API; SQL array assignment
remains pending. Final Jev review selects staged support at 1.0 and the selected
memory correction as verified at probability 0.99. These judgments guide probes
and do not prove correctness or complete a compatibility owner.

Native array/RECORD ABI, bounds, dimensions, general dynamic constructors,
array DML and full collation/resource/restart qualification remain open under
#793/#796/#800/#817/#849. Ordered final results follow below; all Cargo commands
use `CARGO_INCREMENTAL=0`.

Final executed evidence:

- `cargo build --locked --bin cassie`: passed. `cargo test --locked` passes
  library **1,025/0**, analytics **179/0**, backend **2/0** and benchmark
  operations **16/0**, then stops at catalog/security **239/172** after 494.56
  seconds. Exactly one original failure is fixed (option-table populated/NULL
  arrays); no newly failing original identity.
- Fresh `cargo test --locked --no-run --message-format=json` artifacts pass.
  Final new wire matrix: **86/0**; all nine new catalog functions pass in the
  complete catalog suite. Complete parser **299/74**, core wire **74/10**, and
  storage **161 passed/six failed/one ignored** preserve exact failure sets.
  Seven graph-function correctness and four graph SQL checks pass, including
  lateral expansion/explain metrics after restart.
- Full `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  and `cargo fmt --all -- --check`: passed on final source. Preflight
  assert-is-empty diagnostics were corrected without weakening assertions.
- The validator misread a literal opening brace in four new escaping fixtures.
  An equivalent Rust hex escape preserves the actual SQL and every expected
  byte. All 11 touched validators now pass; global validation reports
  **8,767/8,767** compliant source tests. Inventory was refreshed after that
  representation correction and checked again: 95 added identities, zero
  removed or changed existing records. Whitespace and all 29 touched Rust file
  limits pass.

Full repository and hosted green are not established. Native array/RECORD,
collation and complete owner/resource qualification remain open; no owner closes.

## Statement text batch (2026-10-17)

Selected scope: `current_query()` over the existing TEXT identity, with exact
Simple Query message, prepared Parse text, embedded SQL, public analyzed EXPLAIN
and view execution. The native PostgreSQL 18.6 oracle and executable checksum
remain unchanged. New fixtures retain exact descriptors and DataRow bytes for
24 prepared shapes, eight simple messages and four typed parameter shapes;
four signature probes pin native 42883 at Parse. Five wire lifecycle tests cover
cache/comment identity, views, good/error/good recovery, rebinding and paging.
Seven near-code tests cover concurrent cloned sessions, text/scratch admission,
error/cancellation cleanup, final-result-owner release and public analyzed EXPLAIN.

All 24 initial wire witnesses failed before runtime edits with an unknown-function
error. The original current-query fixture also used an empty password, rejected
by the existing listener policy before SQL execution. Its setup now uses the
valid default password; its original descriptor/value assertions remain intact.
The separate trust-identity test remains pending without changing its assertions.

Jev triage rated an introspection gap at 0.83; its implementation review rated
cache/entry scope at 0.68 and memory scope at 0.46, while selecting staged support
at 1.0. Focused probes cover those paths. Public analyzed EXPLAIN reproduced an
actual gap: `WHERE current_query() IS NULL` incorrectly emitted one row because
the separate entry point lacked text. The same execution-local context corrects
it to zero rows; that regression assertion is retained. The large-comment scalar
length probe verifies scratch admission independently of large output retention.
Jev judgments guide probes and do not establish acceptance by themselves.

The context owns admitted SQL and evaluator scratch without mutating shared
session state. Calls through views bypass result caching, while compiled plans
remain reusable at epoch 18. No persistent format/dependency/CI/release/versioning
change is part of this batch. Native NAME/OID/RECORD and full catalog, session and
resource qualification remain open under #793/#817/#821/#849. Ordered final
validation results are recorded below when complete.

Final executed evidence:

- `cargo build --locked --bin cassie` passed. The complete
  `cargo test --locked` run passed library **1,032/0**, analytics **179/0**,
  backend **2/0** and benchmark operations **16/0**, then stopped at
  catalog/security **240/171** after 537.31 seconds. Exactly the original
  current-query failure is fixed; no new failing identity appears.
- Strict Clippy first required extracting the helper from the existing system
  dispatcher. The complete run above includes that extraction. Final consolidation
  of identical zero-arity match arms changes no behavior; after it,
  `cargo build --locked --bin cassie`,
  `cargo test --locked --lib app::query::text_tests` (**7/0**),
  `cargo test --locked --test pgwire_extended catalog_current_query` (**45/0**),
  and `cargo test --locked --test pgwire_extended catalog_runtime_` (**61/0**)
  passed on final source. No lint suppressions or reduced invocation were used.
- Full `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  and `cargo fmt --all -- --check` passed on final source. All 8
  touched-test validators (`cntryl-tools validate-tests -f <path>`) passed;
  global `cntryl-tools validate-tests` reports **8,819/8,819** compliant tests.
- Fresh `cargo test --locked --no-run --message-format=json` artifacts and
  `cargo test --locked --test pgwire_extended catalog_options_` (**86/0**) pass.
  Complete fresh parser **299/74**, core wire **74/10**, and storage
  **161 passed/six failed/one ignored** preserve exact failure sets. Seven
  graph-function and four graph SQL tests pass; the original current-query
  catalog test also passes in its final fresh binary.
- Refreshed `tools/test_inventory.py` and `--check` pass: **45 integration
  identities added**, none removed; two existing records change only source
  line positions. The seven new unit tests are outside that integration
  inventory. Original assertions remain intact. Whitespace and all 21
  touched Rust file limits pass.

Jev's final entry review selects the EXPLAIN correction as fixed at probability
0.97 and staged support at 1.0. Extraction review selects equivalent behavior at
0.95. Actual source/native/Rust checks establish the evidence above; these
judgments do not prove broader correctness. All Cargo uses `CARGO_INCREMENTAL=0`.
Full repository and hosted green remain unestablished; no owner issue closes.


### Selected primitive datum sizes (2026-10-17)

Baseline `61929e4542c8d509556c621d65e6000aa9d7148a`; draft #951 continues
tracking #793/#817. `pg_column_size(any)` now returns the existing INT identity
(OID 23, length 4, typmod -1) for selected existing primitive inputs. Explicit
fixed-width casts use the central type-width registry, rather than guessing from
shared carriers. Unknown strings include a one-byte C-string terminator;
materialized TEXT/VARCHAR and BYTEA casts include a four-byte uncompressed header.
UTF-8 bytes, NULLs, fixed stored-column casts and supported nested cast chains
are qualified. This is not a physical Midge storage-size implementation.

Direct parameters require explicit casts; untyped direct parameters produce
42P18. Unsupported CHAR/NUMERIC descendants, stored varlena provenance, JSON,
arrays, vectors and unqualified expressions remain pending and reject with
0A000 before evaluation. Native NAME/OID/RECORD/array ABI, persistent layouts
and full catalog owners remain unresolved. The plan-cache epoch advances to 19.
Function validation is extracted from its nearly full module; the private query
fixture is shared without changing existing test assertions or configuration.

Red evidence: all 128 initial native literal/NULL wire witnesses failed before
implementation. Green: 155 wire tests plus 32 adversarial nested-cast cases and
eight near-code ownership/resource tests pass (195 new tests). Native PostgreSQL
18.6 covers both output formats and 48 parameter input/output crosses. The
executable SHA256 remains
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Budget denial, cancellation, final-owner release, argument failure and early
rejection of an expanding CHAR cast use the public embedded execution path.

Jev triage selected implementation at 1.0 and flagged provenance at 0.46;
implementation review flagged it at 0.53. Focused native/Rust probes checked
integer carrier collisions, stored fixed casts, parameter formats, Unicode
VARCHAR truncation, BYTEA/TEXT conversions and nested casts. All pass. Final
review selected resolved at 0.60 (confidence 0.40); passing probes, rather than
the judgment, establish the selected behavior. No complete owner closes.

Ordered final validation uses CARGO_INCREMENTAL=0:

- `cargo build --locked --bin cassie` passes.
- `cargo test --locked --test pgwire_extended catalog_datum_nested`: 32/0;
  the earlier `catalog_datum_` run passed 155/0 before these test-only additions.
- `cargo test --locked`: library 1040/0, analytics 179/0, backend 2/0,
  benchmark operations 16/0; catalog/security 241/170 after 461.56 seconds.
  The original exact-value-size failure is fixed; no new failure identities.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  and `cargo fmt --all -- --check` pass.
- `cntryl-tools validate-tests -f <path>` passes for every touched test/fixture;
  global `cntryl-tools validate-tests`, inventory `--check`, and whitespace pass.
- Fresh `cargo test --locked --no-run --message-format=json` artifacts preserve
  parser 299/74, core wire 74/10 and storage 161/six failures/one ignored.
  Current-query wire 45/0, runtime-helper wire 61/0, original query-text unit 7/0,
  seven graph-function cases, four graph SQL cases and the original datum-size
  test pass. Failure sets are compared, not only totals.
- Inventory adds 187 integration identities, removes none and changes no existing
  records. Eight unit tests are tracked separately. Touched files meet size limits.

Full repository green and hosted green are not established. Existing Midge
0.4.1 fresh-store support boundaries remain in force; no migration is performed.


### Ten-batch delivery 1/10: Selected text concatenation (2026-10-17)

Baseline `013d7b6d1446f2958b0b45afeef64afe77d997d8`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-01-*` logs. Current focused wire result: test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 2.46s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 70 (4 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"probe_outcome": {"type": "choice", "choice": "verify", "confidence": 0.91, "probabilities": {"verify": 0.94, "probe": 0.05, "none": 0.01}}}`.


### Ten-batch delivery 2/10: Power and integer remainder (2026-10-17)

Baseline `181c246e5a871e8388cac7f8b71242f4b78fda3a`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-02-*` logs. Current focused wire result: test result: ok. 58 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 7.26s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 67 (7 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"verify": {"type": "choice", "choice": "validate", "confidence": 0.72, "probabilities": {"validate": 0.81, "probe": 0.17, "none": 0.01}}}`.


### Ten-batch delivery 3/10: Integer bitwise operators (2026-10-17)

Baseline `17f176f74ab190d0b871ef8d1aba08b386efba2f`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-03-*` logs. Current focused wire result: test result: ok. 95 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 12.15s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 65 (9 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"verify": {"type": "choice", "choice": "validate", "confidence": 0.71, "probabilities": {"validate": 0.8099999999999999, "probe": 0.17, "none": 0.02}}}`.


### Ten-batch delivery 4/10: Unary math operators (2026-10-17)

Baseline `41b5d59923814c99bb2d3672205fcffd7011c5e9`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-04-*` logs. Current focused wire result: test result: ok. 156 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 18.96s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 62 (12 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"final_review": {"type": "choice", "choice": "validate", "confidence": 0.8, "probabilities": {"validate": 0.87, "probe": 0.12, "none": 0.01}}}`.

Additional final-source unit commands passed in test order before Clippy:
`cargo test --locked --lib generic_prefix` (2/0),
`cargo test --locked --lib rollup_expression::tests` (3/0), and
`cargo test --locked --lib unary_numeric` (8/0). The canonical renderer regression
failed 0/1 before its fix, then all three round-trip tests passed, including four
negative prefix witnesses. This actual caller probe followed Jev's 0.40 probe
probability; the final review selected validation with probability 0.87
(probe 0.12). Earlier mandatory 0.59 integer-boundary and 0.67 prefix-interaction
probes, confirmed findings and fixes are recorded in the dialect profile.
Final parser comparison is 312 passing/62 failing; three additional original
failures are fixed in this slice, twelve cumulatively. No original test assertion
was weakened. Sixty-one new focused wire tests and three new unit tests are added.
Full repository acceptance and the consolidated full-suite gate remain pending.


### Ten-batch delivery 5/10: Boolean truth predicates (2026-10-17)

Baseline `f0e6c98dbd3bd0ed3d67f3cce2610d2a28c5cb37`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-05-*` logs. Current focused wire result: test result: ok. 208 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 31.01s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 61 (13 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"final_review": {"type": "choice", "choice": "validate", "confidence": 0.83, "probabilities": {"validate": 0.89, "probe": 0.1, "none": 0.01}}}`.

Batch 5 also passes `cargo test --locked --lib truth_predicates` (3/0) and
`cargo test --locked --lib rollup_expression::tests` (4/0), before the final parser
comparison and strict analyzers. The depth regression failed independently before
its fix. The final full parser comparison was rerun after the CASE extractions;
313/61, with no new failure identity. Three new unit tests and 52 wire tests are
added without changing original assertions. Mandatory Jev probes at 0.75 and 0.62
and the final 0.89 validation judgment are recorded in the dialect profile.
The user has now authorized continuing coherent batches until the full test suite
and required analyzers pass, including further batches beyond the initial ten if
needed. Required gates and native semantic boundaries remain in force.


### Ten-batch delivery 6/10: Symmetric BETWEEN (2026-10-17)

Baseline `8a205b55462775e8a09cb0a8e8cd90217bbc57b5`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-06-*` logs. Current focused wire result: test result: ok. 249 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 31.27s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 60 (14 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"final_review": {"type": "choice", "choice": "validate", "confidence": 0.74, "probabilities": {"validate": 0.83, "probe": 0.16, "none": 0.01}}}`.

Selected symmetric range scope: `BETWEEN SYMMETRIC`, `NOT BETWEEN SYMMETRIC`,
and explicit `ASYMMETRIC` over the existing selected scalar contracts. A compact
AST flag preserves PostgreSQL's comparison order and lazy Boolean rewrite without
copying operand trees. False flags are omitted and defaulted in serialized AST and
normalized feedback shapes; existing serialized ordinary predicates retain their
shape. Semantic plan epoch is 25. Normal index, aggregate, projected-read and
timestamp bound extraction explicitly declines symmetric predicates; reversed
bounds produce the same native rows before and after creating a scalar index.

The focused batch adds 41 wire tests (23 scalar witnesses, five parameter fixtures
covering 20 input/output format crosses, 12 folding/NULL/row/aggregate/EXISTS
interactions and an eight-execution index/scan witness). Six focused unit checks
cover compact 16-level nesting, legacy serialization and mapping, combined cast
nesting, planner and runtime deadline interruption with released ownership, and
staged array rejection. Existing canonical aggregate SQL roundtrips now include
both symmetric forms. Array, JSON and vector comparisons reject deterministically
with 0A000 and remain pending under #807/#809/#793; the captured native array
result is an open qualification obligation, not a passing Cassie cell. Complete
row, volatile-function and native type qualification remains open.

The resource review probability 0.30 prompted a runtime session constant-pruning
probe. It exposed a missing per-node interruption path; both planning and runtime
constant walks now check their execution controls. Deadline tests start active
controls and expire during repeated comparisons, rather than checking only an
already-expired entry point. The initial 0.79 sibling gap prompted NULL,
short-circuit, parameter and indexed-versus-scan witnesses. All original assertions
remain intact. Full repository green and owner closure are not established here.


### Ten-batch delivery 7/10: Function-style primitive casts (2026-10-17)

Baseline `69fb17cbdfdcd3665745ebf12eee0354dfbdf7e4`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-07-*` logs. Current focused wire result: test result: ok. 344 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 55.55s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 58 (16 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"final_review": {"type": "choice", "choice": "validate", "confidence": 0.79, "probabilities": {"validate": 0.86, "probe": 0.14, "none": 0.0}}}`.

Selected type-function scope: unmodified, one-argument `int2`, `int4`, `int8`,
`float8`, `bool`, `text`, `date`, `uuid` and `bytea`, including their `pg_catalog`
forms, plus qualified `pg_catalog.time`, `pg_catalog.timestamp` and
`pg_catalog.varchar`. Native bare TIME/TIMESTAMP/VARCHAR function notation is not
selected. Checked scalar casts retain exact integer widths, strict NULL behavior,
IEEE special values, canonical UUID/bytea values and native default column labels.
DATE from existing TIMESTAMP retains the source identity required for extraction.

Unknown string/NULL constants invoke native type coercion before user routine
lookup. Exact typed user signatures remain callable, including non-strict typed
NULL input. Unknown parameters retain overload errors (42725/42883), supported
TEXT/VARCHAR inference and native distinction from explicitly declared inputs.
A bounded, controlled wire Parse normalization preserves irreversible unknown
textual-parameter coercions before effective OIDs erase their origin. Native
36-cross user-text-routine probes cover direct, nested, CTE, EXISTS and INSERT
callers; the Rust regressions preserve those choices. Normalized coercions use
existing Cast AST nodes; catalog-sensitive calls retain existing rebind behavior.
Complete routine overload/body/prepared lifecycle qualification remains open.

This increment adds 95 focused wire tests. The cumulative selected result is
344/0. Five new resource units, plus one existing selected-lane literal unit in
the `scalar_literal` filter, pass 6/0. They verify copied diagnostic ownership,
exact/minus-one complete error-frame budgets, coercion scratch retirement,
unchanged AST on allocation denial and cancellation before mutation/allocation.
Native literal diagnostics distinguish integer input range messages from FLOAT8
range messages; their UTF8 input copy and encoded frame are admitted before
allocation. UUID input uses a fixed 16-byte buffer and accepts the captured native
simple, braced, uppercase and four-digit hyphen forms, rejecting malformed forms.

The DML sibling probe exposed the existing literal-only INSERT VALUES path.
Computed scalar expressions now use the shared evaluator before insertion;
literal conversion is preserved. Native-backed cases cover arithmetic, CASE,
truth/symmetric predicates, typed routine resolution and errors before any row is
written. Aggregate VALUES rejects 42803; column/subquery VALUES remains pending.
No complete DML, volatility or all-function resource qualification is inferred.

NUMERIC, array, JSON and vector conversion obligations remain explicitly pending;
they are not silently relabeled as primitive results. Unknown DATE/TIMESTAMP
function parameters select native TIMESTAMPTZ1184, whose input contract remains
staged with 0A000; DATE1082 is not substituted. Native pending NUMERIC arithmetic
and temporal-input receipts are retained separately from passing cells. Semantic
cache epoch is 26; there is no new persistent scalar or storage migration.

TDD/probe record: original two tests red at `69fb17cb`; an early 42-failure wire
run exposed literal/parameter confusion, then six failures identified source-aware
DATE and exact diagnostics. Native textual-parameter shadowing exposed an additional
Parse choice, and the DML caller probe exposed computed VALUES materialization.
All confirmed selected findings are fixed. Initial Jev sibling gap 0.82 prompted
parameter/routine probes; resource gap 0.30 prompted the cancellation probe. Final
review selected validation with probability 0.86 (probe 0.14). Full owner closure,
complete repository green and hosted acceptance remain pending.


### Ten-batch delivery 8/10: LIKE ESCAPE (2026-10-17)

Baseline `6910381ef1ad5e21ead10050876aaddadeeeb640`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-08-*` logs. Current focused wire result: test result: ok. 420 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 70.94s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 57 (17 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"next_action": {"type": "choice", "choice": "validate", "confidence": 0.85, "probabilities": {"validate": 0.9, "probe": 0.1, "none": 0.0}}}`.


Batch 8 qualifies 76 new native-backed wire tests, bringing the cumulative
selected scalar result to 420/0, plus seven matcher unit tests (two new).
The compact LIKE ESCAPE AST carries value, pattern and escape separately; the
matcher borrows their text, accepts an empty or single Unicode escape character,
and checks interruption during backtracking and exhausted-input percent tails.
Native strict `like_escape` ordering is retained before the outer strict LIKE:
invalid constant escape still errors with a NULL left operand, while a NULL
pattern suppresses helper validation. CHAR pattern and escape operands coerce
to TEXT with trailing blanks trimmed; the CHAR value keeps its padding.
Twenty-four parameter input/output format crosses retain TEXT/VARCHAR/CHAR
identities and native unknown-input inference. Exact 22025/42883 errors and
Describe(statement)-before-Bind-error frame ordering are qualified.

Initial Jev gaps 0.80 and 0.74 prompted the NULL/row, coercion, precedence,
malformed-suffix, parameter, protocol-phase and interruption witnesses. Five
unparenthesized chained LIKE/NOT LIKE syntax regressions fail before the parser
guard and pass afterward; two parenthesized forms remain valid. The shared
default matcher now returns FALSE when exhausted input prevents reaching a
trailing escape, matching the native witness; a reached escape still errors.
Final review selected validation 0.90/probe 0.10, and every pending selected gate
subsequently passed. Full parser result is 317/57, with the original ESCAPE
failure fixed and no new failure identity. ILIKE, collation-aware matching,
BYTEA/array/JSON/vector operator contracts and broader mixed IN/BETWEEN predicate
precedence remain unqualified obligations under #796/#793. Native mixed-level
observations are retained in `target/ten-batch-08-native-level-probe.json` for
that owner follow-up; this finite increment does not close those owners.


### Ten-batch delivery 9/10: Identifier byte boundaries (2026-10-17)

Baseline `a8bbff8430a25a9a881bd09f0ab7e376752c0b6a`. Selected existing scalar/syntax support under #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-09-*` logs. Current focused wire result: test result: ok. 471 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 76.57s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 56 (18 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"next_action": {"type": "choice", "choice": "validate", "confidence": 0.45, "probabilities": {"validate": 0.56, "probe_metadata": 0.05, "probe_scope": 0.16, "probe_delimiters": 0.08, "none": 0.15}}}`.


Batch 9 qualifies 51 new wire tests in the cumulative 471/0 main run, plus three
supplementary parameter tests with 12 text/binary input/output combinations on
unchanged production source. All supplementary build, test, full strict Clippy,
formatting, validators and inventory checks pass. Three identifier boundary
units pass. The complete parser result is 318/56 with no new failure identity;
the original UTF-8 identifier truncation failure is fixed.

SQL identifier components retain PostgreSQL's UTF-8-safe 63-byte prefix after
quote decoding. The bounded decoder still scans the full token for its closing
quote. Unquoted folding, U&/UESCAPE, doubled quotes, qualifiers, output names,
CTE column lists, and table/column/index/sequence declarations have selected
native witnesses. Qualified sequence components truncate independently. Existing
stored-metadata constructors and generated internal column qualifiers retain
exact spelling through a separate internal parse path. Column collisions report
42701; derived ambiguous columns report 42702. This qualifies their SQLSTATEs,
not complete utility error timing or truncation NOTICE output.

Jev gaps 0.71 and 0.51 prompted native sibling probes and three focused boundary
units. A further probe judgment of 0.51 exposed two failing delimited derived
aliases. Reusing the existing alias namespace binder retained delimiter identity;
subsequent descriptor probes exposed a TEXT fallback, fixed by extending the
existing derived-source schema inference to that wrapper. Uppercase letters,
doubled quotes, spaces and dots now retain native INT4 descriptors and both
payload formats. Two additional enclosing-CTE probes failed before CTE schemas
were passed into alias column inference and pass afterward, including wildcard
projection. Three parameter probes preserve inferred INTEGER, declared CHAR(3)
with native typmod, and CTE-backed INTEGER metadata. Final review selected
validation 0.56 (metadata probe 0.05, scope probe 0.16, delimiter probe 0.08,
none 0.15). These judgments guide probes; actual source and passing tests are
acceptance evidence. Full NAME identity, NOTICE and utility phase qualification
remain under #793/#796; no owner closes or persistent layout changes here.


### Ten-batch delivery 10/10: CTE materialization and integer dependency scope (2026-10-17)

Baseline `4a171c57ce119f74792bf92bb9998da0128bbeb0`. Selected existing scalar/syntax support under #801 and #796
with no new persistent type or storage migration. Native PostgreSQL 18.6
reference descriptors and both wire payload formats are retained in focused
Rust tests. TDD red evidence and targeted Jev probes are preserved in the ignored
`target/ten-batch-10-*` logs. Current focused wire result: test result: ok. 527 passed; 0 failed; 0 ignored; 0 measured; 2924 filtered out; finished in 86.40s.

Build, focused locked tests, complete parser regression comparison, full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, refreshed inventory and whitespace checks completed. Parser
failures remaining: 54 (20 fixed cumulatively from
the 74-failure starting baseline), with no new failure identity. The consolidated
full repository gate follows batch 10 under the documented local delivery policy.
Known unrelated failures and complete owner acceptance remain pending.

Jev review and probe outcome: `{"next_action": {"type": "choice", "choice": "resource", "confidence": 0.83, "probabilities": {"validate": 0.06, "probe": 0.07, "resource": 0.87, "none": 0.0}}, "gap": {"type": "noul", "noul": 0.56}}`.

Batch 10 retains the explicit CTE materialization mode in the public AST, with
legacy-compatible serde defaults. MATERIALIZED remains a planning barrier;
finite nonvolatile NOT MATERIALIZED definitions expand under cancellation,
deadline, memory and expression-depth controls. Volatile and recursive inputs
retain sharing. Nested CTE/EXISTS scope expansion, mixed wildcard alias cases,
recursive SEARCH/CYCLE and data-modifying CTE owner acceptance remain pending.

The 53 new native PostgreSQL 18.6 wire regressions cover mode semantics,
repeated references, supported volatile setting effects, error phases, aliases,
duplicate output rejection, arithmetic dependency types, recursive anchor
widths and enclosing seed scopes in both result formats. Initial binding now
uses enclosing CTE row schemas. Recursive binding derives its row types from
the bound anchor. WITH RECURSIVE accepts ordinary seed definitions without a
UNION. The latter two refinements were driven by red native wire reproductions.

Jev's resource/gap judgments (0.87/0.56) prompted duplicate-name probes and a
wide dependent-scope budget/atomicity test. Six optimizer control tests cover
initial and post-clone budget denial, wide dependent copies, depth, cancellation
and deadlines. The cache ownership concern (0.32) prompted explicit isolation
of the final-owner lifetime test from optional result caching, plus the six
existing cache ownership regressions. The refinement anchor concern (0.58)
prompted the two ordinary-seed and quoted-column reproductions. All judgments
and raw probes remain in ignored target/ten-batch-10-* artifacts; their
probabilities do not prove correctness or close the full #801 contract.

Eight broken documentation links now use current test paths or immutable
historical workflow references. Historical workflow interfaces are labeled
archived. No CI, release or versioning configuration changed.


### Autopilot consolidated inventory after the first ten batches (2026-10-17)

Immutable source baseline `c550d1fcc4e971043d741e63ad099a2851d85f90`, with a clean
worktree throughout the run. Ordered build passed, then
`CARGO_INCREMENTAL=0 cargo test --locked --no-fail-fast` returned failure.
Observed case totals: **7,211 passed, 2,480 failed, 9 ignored and 1 incomplete**.
These are actual default-invocation counts; no result is inferred for other
feature configurations or the separate Python tests.

| Target | Passed | Failed | Ignored | Incomplete |
| --- | ---: | ---: | ---: | ---: |
| lib | 1068 | 3 | 0 | 0 |
| analytics | 179 | 0 | 0 | 0 |
| backend_ci | 2 | 0 | 0 | 0 |
| bench_operations | 16 | 0 | 0 | 0 |
| catalog_security | 241 | 170 | 0 | 0 |
| compatibility | 56 | 644 | 8 | 0 |
| configuration_isolated | 20 | 0 | 0 | 0 |
| constraint_ingress | 3 | 0 | 0 | 0 |
| domain_models | 94 | 0 | 0 | 0 |
| executor | 108 | 3 | 0 | 0 |
| expression_index_upsert | 6 | 1 | 0 | 0 |
| json_writes | 23 | 0 | 0 | 0 |
| metrics | 59 | 2 | 0 | 0 |
| null_safe_predicates | 49 | 0 | 0 | 0 |
| parser_types | 320 | 54 | 0 | 0 |
| pgwire_core | 74 | 10 | 0 | 0 |
| pgwire_extended | 2623 | 827 | 0 | 1 |
| planner_controls | 126 | 0 | 0 | 0 |
| query_scan_controls | 61 | 1 | 0 | 0 |
| relational_qualification | 77 | 1 | 0 | 0 |
| rest | 101 | 0 | 0 | 0 |
| search | 64 | 1 | 0 | 0 |
| sql_contextual_literals | 7 | 0 | 0 | 0 |
| sql_derived_sources | 14 | 0 | 0 | 0 |
| sql_exists_subqueries | 23 | 35 | 0 | 0 |
| sql_mutations | 276 | 35 | 0 | 0 |
| sql_queries | 1151 | 687 | 0 | 0 |
| sql_relation_identifiers | 15 | 0 | 0 | 0 |
| sql_set_operations | 15 | 0 | 0 | 0 |
| storage_indexes | 161 | 6 | 1 | 0 |
| temporal_predicate_literals | 4 | 0 | 0 | 0 |
| time_series_float_partitions | 4 | 0 | 0 | 0 |
| vector_embeddings | 171 | 0 | 0 | 0 |

The extended pgwire target completed 3,450 cases; its final ordered-set budget
case remained in fixture seeding. A native process sample identified repeated
committed-row scans through `find_document_id_by_fields` while inserting the
50,000 primary-key rows. The isolated worker was terminated to allow remaining
no-fail-fast targets to finish. Its result is **incomplete**, not passed and not
an asserted timeout failure. The scan path and unchanged workload are tracked
in [#952](https://github.com/cntryl/cassie/issues/952).

Raw output and case/source grouping are retained in ignored
`target/autopilot-full-after-10-*` and `target/autopilot-failure-groups.json`.
The three library failures expect pre-normalization long SQL identifiers;
native PostgreSQL 18.6 confirms the 63-byte public name. Resource fixtures must
retain their original long owned names through programmatic AST construction,
while SQL-facing assertions follow that native boundary. Separate reference
probes confirm scientific FLOAT8 output and successful nonfinite float casts;
those do not justify changing unrelated runtime-error expectations. Full
repository acceptance remains pending.


### Autopilot batch 11: Preserve identifier resource coverage (2026-10-17)

The three library failures above are resolved without changing production
behavior. SQL-facing positional aliases assert the native 63-byte name. Long
resource fixtures now inject the original 4,096-byte and 131,072-byte names
through the programmatic AST before binding. Existing typed NULL, output
metadata, retained-memory, budget-denial and drop-release assertions remain.
A new counterprobe separately asserts the normalized SQL alias fits its
16-KiB budget and releases its analysis memory. No native array OID promotion
is claimed by this identifier-only correction.

Ordered build and full library test pass (1,072/0), as do the 54 batch-9
identifier wire cases. A test-only missing semicolon was corrected after
Clippy; the affected eight Parse-analysis tests then pass in a supplementary
ordered build/test/strict-Clippy/format gate. Full strict workspace/all-target/
all-feature pedantic Clippy, formatting, touched/global test validators,
inventory, documentation and whitespace checks pass. Production source is
unchanged, so the earlier 527-case cumulative wire checkpoint remains the
production reference; it is not reported as a newly repeated run here.

Jev's fixture concern (0.44) prompted the normalized SQL versus large owned
AST counterprobe. Final review selected validation (0.91), admission probe
(0.01), lifetime probe (0.01), insufficient evidence (0.07); all final gates
subsequently completed. Raw native receipts and red/green logs are retained in
ignored `target/autopilot-identifier-*` and `target/autopilot-batch-11-*`. The
consolidated baseline remains red; no complete owner or repository acceptance
is claimed by this slice.


### Autopilot batch 12: Published uniqueness owners and index errors (2026-10-17)

[#952](https://github.com/cntryl/cassie/issues/952) replaces repeated committed-row
validation scans with the existing unique scalar-index reservation owner lookup.
Primary constraints retain their declared error name. Staged owner deletion,
re-keying and excluded updates are reconciled against the borrowed transaction
snapshot. Matching physical kind/uniqueness/fields/expressions/predicate are
required before a reservation miss is authoritative. Missing or divergent
physical definitions use scan validation. Ordinary single-column UNIQUE also
retains its original fallback until
[#953](https://github.com/cntryl/cassie/issues/953) qualifies field backing. No new
metadata keyspace, storage layout, format migration or Midge abstraction is added.

The original four scan probes were red at 32, 64, 97 and 129 read entries. Fresh
validation is now measured inside an explicit transaction and requires zero
committed scans; the separate once-per-commit cardinality pass is bounded to
33 entries for the 33-row fixture. All 17 mutation probes pass, including
plain-UNIQUE committed/staged/NULL controls, canonical CHAR, partial predicates,
excluded owners and missing/divergent physical definitions. The divergent
definition probe was red before the physical guard.

A prototype that trusted every field reservation miss was rejected: raw
constraint metadata had one record while its canonical lookup had zero, a
committed plain-UNIQUE duplicate was admitted, and a named PK reported the
generated index name. Permanent controls now protect the retained fallback.
That prototype was never committed or pushed. Its namespace/backing prerequisite
is explicitly recorded in #953, rather than treated as a proven publication.

NULL index keys preserve partial-predicate evaluation. Runtime expression and
predicate errors propagate through the typed QueryError conversion instead of
becoming Parse errors. Two wire probes were red (missing error or 42601) and
now preserve the native 22012, failed transaction state, rollback and empty
recovery result. Pinned PostgreSQL 18.6 receipts cover both paths.

Ordered build and library tests pass (1,072/0). Full mutation tests are 293/35
with the same 35 failure identities; expression-index tests remain 6/1 with the
same failure. The affected wire filter is 2/8 and index-query filter 22/4, with
no newly failing identity against the consolidated baseline. Full strict
workspace/all-target/all-feature pedantic Clippy, formatting, touched/global
test validators, inventory, documentation and whitespace checks pass. These
known-red suites are not reported as green.

The unchanged 50,000-row ordered-set budget case now completes in 14.50 seconds
on final source and reaches a real control-query parser failure: 42601,
`unexpected text after function call` for WITHIN GROUP. Its previous incomplete
result is now qualified as a separate failing parser case under
[#806](https://github.com/cntryl/cassie/issues/806); aggregate/resource support is
not claimed complete. The original workload and assertions are unchanged.

Jev's gap 0.46 prompted physical publication probes; the confirmed divergent
definition finding was fixed. Prior overlay concern 0.98 drove the staged-owner
controls. Final review selected validation 0.89, provenance 0.04, overlay 0.01,
errors 0.00 and insufficient evidence 0.06. Actual probes and required analyzers
support this scoped outcome. Raw logs remain in ignored
`target/autopilot-batch-12-*`; no full repository or owner acceptance is claimed.


### Autopilot batch 13: Native conditional FLOAT8 fixtures (2026-10-17)

This test-only correction follows pinned PostgreSQL 18.6 scientific FLOAT8
text and successful Infinity, -Infinity and NaN casts. Exact binary float
payloads, OID 701, size 8 and integer-only expectations remain asserted.
An arithmetic counterprobe distinguishes promotion before subtraction from
integer subtraction before promotion. Invalid float text still requires
22P02 with no rows or completion and idle recovery. The real empty-table
GREATEST integer/boolean type-error failure is unchanged.

The previous COALESCE filter was 3 passing/5 failing. Ordered build, all nine
COALESCE tests, conditional descriptor test, full strict workspace/all-target/
all-feature pedantic Clippy, formatting, touched/global test validators,
inventory, documentation and whitespace checks pass. No production source
changed; the earlier full-library and cumulative wire results are references,
not repeated runs. Full repository acceptance remains pending.

Jev's initial gap 0.35 prompted the arithmetic timing counterprobe against
native receipts. Final review selected checkpoint 0.93, arithmetic 0.05,
nonfinite 0.01 and insufficient evidence 0.01. Raw receipts and ordered logs
remain in ignored `target/autopilot-conditional-*` and
`target/autopilot-batch-13-*`.


### Autopilot batch 14: Conditional binding diagnostics (2026-10-17)

GREATEST/LEAST incompatible admitted families now report native 42804 and
common-type names. NULLIF reports the missing equality operator signature
and 42883. The existing common-type algorithm and admitted families are
unchanged; only the generated diagnostic and its SQLSTATE routing change.
The six original new tests were red with 42601 before implementation.

Eight new tests cover 171 error witnesses: every existing non-Boolean family
against Boolean in both orders, mixed integer/FLOAT before the incompatible
argument, empty/populated input, transaction abort/rollback, explicit casts,
declared parameter signatures, Parse-before-Bind errors, and 15 nested or
mutation caller contexts. Native PostgreSQL 18.6 receipts retain exact states
and messages. All 13 related wire tests pass. Full library passes 1,072/0;
parser/types remains 320/54 with no new failure identity. Related SQL
conditionals are 30/1 with the same pre-existing explicit FLOAT overflow
failure. Native `GREATEST(CAST(1e308 * 10 AS FLOAT),0)` returns 22003, while
Cassie returns Infinity; this assertion is preserved for the numeric owner.

Initial Jev gap 0.58 prompted reverse-order and merged-type probes. Review
orientation 0.32 and propagation 0.36 prompted width and nested/DML probes.
Residual propagation 0.34 prompted EXPLAIN, RETURNING, UNION, correlated
EXISTS, ORDER BY and HAVING probes. The final residual propagation judgment
0.42 prompted 30 declared-parameter Parse probes. All probes pass; no runtime
change was needed beyond the diagnostic correction. The judgments are retained
as uncertainty, rather than represented as independent correctness proof.

Full strict workspace/all-target/all-feature pedantic Clippy passes after
correcting a test helper to use array chunks. Formatting, touched/global test
validators, inventory, documentation, module sizes and whitespace checks pass.
After test-only probe expansion these checks were repeated in an ordered
build/test/strict-analyzer gate; production source did not change after the
full-library run. Benchmark policy validation is blocked by the pre-existing
`.cntryl/repository.toml` reference to missing `.github/workflows/bench.yml`.
No CI, release, versioning or validator configuration was changed. The blocked
benchmark check and red suites are not passes; full acceptance remains pending.
Raw evidence remains in ignored `target/autopilot-batch-14-*`.


### Autopilot batch 15: Qualified operators and BYTEA concatenation (2026-10-17)

Qualified pg_catalog operators now parse at PostgreSQL's generic precedence,
including mixed bare/qualified arithmetic, comparisons, left associativity and
unary chains. Existing checked scalar nodes execute them. The selected symbols
and staged unsupported namespaces/overloads are recorded in the dialect profile;
full #796 acceptance is pending. The initial native group was 2 passing/35
failing before implementation; mixed-precedence and BYTEA expansion was 48/23.

BYTEA `||` selects the new public `BinaryOp::ByteaConcat` before shared string
carriers erase its type. Existing OID 17 and canonical hex row representation
remain unchanged. Unknown literals/parameters anchored by BYTEA coerce to BYTEA;
known TEXT/VARCHAR operands retain their text overload. Native malformed BYTEA
input errors occur during Parse, while identical poisoned explicit TEXT values
succeed as TEXT. The checked kernel joins hex payloads under one prefix.
Expression/output inference, parameter analysis, VALUES binding and canonical
rendering retain the selected mode. Legacy Concat JSON remains unchanged; the
new mode round-trips independently. Downstream exhaustive Rust BinaryOp matches
must handle the new variant. Rebuildable plan-cache epoch is 30.

One hundred new tests cover 82 wire declarations, six direct SQL tests, five
parser integration controls and seven units. Native receipts qualify 76 value
witnesses in both result formats, 20 parameter-format crosses and six invalid
literals through Parse and simple query. Direct queries cover unknown/text
origin, CTE/derived/mutation/RETURNING propagation, cancellation, atomic row
admission and cache-disabled final-owner release. All new tests pass.

Jev's precedence concern 0.79 prompted 18 mixed native witnesses. Its BYTEA
parameter concern 0.80 prompted inferred/declared signatures and text/binary
crosses. Qualified-name concern 0.96 prompted extra lexical probes: the first
member-name guard fixed public.operator(2), then a quoted-schema matrix exposed
"public".operator(2) being classified as a column. Both were red before their
fixes and have permanent regressions. The SQL quoted-chain parser now excludes
actual function calls while preserving parentheses inside delimited column
names. Final concerns 0.38 (literal origin) and 0.33 (qualified names) prompted
eight computed-TEXT poison controls across wire/direct queries and five more
schema/function-name parser controls. All pass; no remaining confirmed finding
is concealed by the judgments. Full UDF/language acceptance is not claimed by
function-call parser classification probes.

The combined wire gate passes 614/0 and full library 1,079/0 before the final
five-line quoted-function boundary refinement. On final production source, an
ordered build/test gate passes 128 affected identifier/operator wire cases and
38 parser/string-operator/renderer units. After test-only probe expansion, all
82 operator wire tests pass. Parser/types is 328/51 with exactly the prior
failure identities minus the two qualified-operator failures and BYTEA cast/
concatenation case. The BYTEA SQL filter is 269/32, with the prior failure
identities minus its concatenation comparison case. These known-red suites are
not passes. The full-library and 614-case results are not misrepresented as
newly repeated after the final parser refinement.

Final full strict workspace/all-target/all-feature pedantic Clippy, formatting,
touched/global test validators, inventory, documentation, module sizes and
whitespace checks pass. The existing missing bench.yml benchmark-policy blocker
recorded in batch 14 remains; no CI/release/versioning or validator configuration
changed. Full repository acceptance remains pending against the immutable
consolidated baseline. Raw native receipts, red/green logs, exact failure-ID
differences and review requests/results remain in ignored
`target/autopilot-batch-15-*` and `target/autopilot-qualified-operator-*`.

## Autonomous continuation batch 16: fixed BYTEA routines

The existing canonical-hex OID17 carrier now supports encode/decode (hex,
base64, escape), get_byte/set_byte, get_bit/set_bit, bit_count(bytea), and
sha224/sha256/sha384/sha512, including local and pg_catalog names. Byte indices
use INT4; BYTEA bit indices use INT8. set_byte retains the low eight bits.
Native Base64 padding-state/newline behavior and whole-character UTF8 hex
errors are preserved. Typed bounds and ambiguity errors retain 2202E and 42725.
Rebuildable plan-cache epoch is31; stored row layout is unchanged.

The initial coherent wire chunk reproduced140failures/1pass. Follow-up native
probes found an escape-octal overflow, missing scratch admission and declared
function-name/arity collisions; each has permanent regressions and passing
checks. Constant scalar modifiers retain the existing admitted42809 path.
Complete function-signature messages are allocated only after query admission.
Codecs, bit scans and streaming hash updates check interruption; result-owner
release and scratch-budget/cancellation controls pass.

There are179new test declarations:171wire,3direct SQL and5units. Native18.6
receipts pin95codec/access/hash values in both formats,11parameter signatures
through44input/result crosses,41Parse/Bind diagnostics,13empty-input cases,
additional overload and scoped-routine controls. BIT-selected overloads remain
explicitly pending; their rejection controls are not native acceptance cells.
Unknown bit_count is ambiguous; an unknown first get_bit/set_bit argument with
an INT8 index selects BYTEA, while INT4 selects the separate pending BIT family.

Jev's resource0.36, binding0.39 and codec0.41 gaps prompted focused probes.
Two scratch tests were red before controls reached evaluation and green after;
cancellation after admission releases the owner. Declared-name and distinct-
arity probes each reproduced a failure before their fixes. Extra UTF8/whitespace
codec witnesses pass. The typed-index follow-up0.87 prompted two more inferred
BYTEA signatures and eight format crosses, all passing. These judgments guide
probes; they do not establish full owner acceptance.

Final production validation passes1,084library tests and171new wire tests.
The earlier combined scalar/conditional/operator/new-routine run passes793/0.
The existing BYTEA wire group is9passed/33failed, fixing eight identities from
its41-failure consolidated baseline. BYTEA SQL is280passed/24failed, fixing
exactly eight prior failures with no newly failing identity. All three new
SQL tests pass. Parser/types remains328/51 with the same failure identities.
These known-red suites and the consolidated full baseline remain red.

Full strict workspace/all-target/all-feature pedantic Clippy, formatting,
touched/global test policies, refreshed inventory, docs, module sizes and
whitespace checks pass. A test-only name-policy correction was executed under
its final identity, followed by the analyzers and policies again. The existing
benchmark-policy check still fails because .github/workflows/bench.yml is
absent; no CI/release/versioning or validator configuration changed. #816 and
all full-owner obligations remain open. Raw receipts and exact failure-ID
comparisons are retained in ignored target/autopilot-batch-16-* artifacts.

### Autopilot batch 17: Fast correctness fixtures (2026-10-17)

The user requested that correctness tests stay small and that performance/stress
work remain in benchmarks. This slice adds explicit ephemeral Cassie construction
using the existing direct Midge in-memory mode, and a server entrypoint that owns
an already-bound listener. Both retain production SQL, authentication, TLS,
listener validation, connection admission, and execution behavior. No storage
format, CI, release, versioning, or benchmark ownership changes are introduced.

The shared `parameter_occurrence_wire::run_script`, `wire_queries::run_cycles`,
and generic `sql_fixture`/`sql_fixture_with_config` fixtures now select memory
explicitly, without changing global storage variables. The generic SQL fixture
previously forced local mode at nearly 1,900 source call sites; its directory is
private and never exposed to callers. `local_sql_fixture_with_config` preserves
local mode explicitly for persisted-portal calibration and teardown tests.
The ephemeral helpers previously created and deleted a fresh local directory for
every invocation; their ordinary query callers never reopened that private store. Local restart, recovery,
storage-layout, and persisted-portal fixtures remain local. The memory regression
creates independent same-named tables with different rows and verifies that a fresh
instance cannot read a dropped instance's table, even with local storage selected
in the environment.

Shared pgwire, collection-resource, portal-memory, unique-conflict, ordered-set,
and TLS fixtures retain their original bound listener instead of dropping and
rebinding it, eliminating a port handoff race and fixed 50 ms startup sleeps.
New listener regressions cover immediate authenticated use and graceful shutdown,
actual wildcard-address transport rejection, and invalid TLS identity rejection.
They initially failed compilation because the listener APIs were absent; all three
subsequently passed. Existing negotiated-TLS and TLS cancellation checks exercise
the new owned-listener entrypoint too.

Ordinary `write_frames` cycles coalesce frontend frames into one write, preserving
frame bytes/order and explicit cycle boundaries. Fragmentation tests still send
fragments themselves. All original Parse/Bind Flush, Sync, descriptor, exact
text/binary payload, and recovery assertions remain. A trial pipelining refactor
was removed after measurement failed to improve total suite time; no intermediate
ReadyForQuery frames are manufactured.

Seven resource-budget inputs are smaller: ordered-set FLOAT8 rows fall from
50,000 to 1,024 (8 KiB payload before overhead against a 4 KiB budget), and six
collection aggregates use 64 512-byte values (32 KiB payload). Small positive
controls, same-input nonretaining controls, SQLSTATE 54000, no partial rows, and
same-session recovery remain. Pending aggregate features remain known failures.
Large cancellation fixtures are not blindly shrunk: their timing races require
execution-boundary synchronization before replacement; [#954](https://github.com/cntryl/cassie/issues/954) tracks those exact fixtures. The standing test workload
contract is in `AGENTS.md` and `docs/performance-contracts.md`.

Jev's initial listener gap probability 0.45 prompted actual-address/TLS/shutdown
probes; concrete-patch review returned 0.20. Memory migration review returned a
0.34 gap, prompting the unchanged 793-case wire lane, independent-instance memory
regression, and local bootstrap/reopen/cleanup probes. The initial full storage
run had six pre-existing failures plus a new fixture assertion that expected the
legacy CollectionNotFound variant; the binder actually returns the typed
CatalogObjectNotFound(Relation), and that expectation was corrected. The
independent-instance and 14 local probes then passed. Shared SQL fixture review
returned a 0.35 gap; the full SQL suite and persisted-portal probe address it.
Model judgments are not correctness
proof. Raw requests/responses and measured process sampling are retained under
ignored `target/fast-tests-*`.

Measured execution, excluding compilation: the exact same 793 passing wire
identities fell from 115.69 seconds with the local fixtures to 11.29 seconds with
explicit memory fixtures (10.25x in these runs). All original SQL, descriptors,
payloads and Flush cycles remain. This is developer-feedback evidence on this
host, not a statistical performance benchmark or production latency claim.

The full SQL run returned 1171 passed / 676 failed in 149.44 seconds, versus the
historical after-batch-10 run's 1151 / 687 in 338.51 seconds. These full SQL runs
have different case counts and prior compatibility fixes; do not call that a
matched performance experiment. Three failures newly observed relative to that
older baseline used unchanged, explicitly local constructors and expected legacy
Lower diagnostics or allowed an invalid lower(integer) in a dead COALESCE branch.
PostgreSQL 18.6 rejects those signatures with 42883 before execution. Their
fixtures now assert exact native diagnostic messages and use a valid row-dependent
TEXT division-by-zero fallback to prove lazy COALESCE, with a NULL-first negative
control. All three pass in 0.02 seconds after that test-only correction. No runtime
implementation was changed to make them pass. The full SQL suite was not repeated
after those three fixture-only edits. Native frames and Jev review (gap 0.14) are
retained in target/fast-tests-scalar-*.

Final ordered checks:

| Command / lane | Result |
| --- | --- |
| `CARGO_INCREMENTAL=0 cargo build --locked` | Passed |
| `cargo test --locked --test storage_indexes -- in_memory:: support_sql_fixture::teardown_tests midge_layout_bootstrap` | 15 passed; includes real local reopen/cleanup |
| `cargo test --locked --test pgwire_extended -- bytea_functions scalar_batch_ qualified_operators conditional_` | 793 passed in 11.29 seconds; exact passing identities match the local-fixture run |
| `cargo test --locked --test pgwire_core` | 77 passed / 10 pre-existing failures; TLS and all three owned-listener regressions pass |
| Seven narrowed aggregate memory-budget cases | Seven known feature failures in 1.81 seconds; original ordered-set incomplete case now completes with its pending grammar failure; no new failing identity |
| Persisted named-portal memory lifecycle case | Passed on explicit local storage |
| Four unique-conflict / row-lock cancellation cases | Four pre-existing failures; no new failing identity |
| Three corrected scalar contracts plus four local fixture teardown checks | Seven passed in 0.38 seconds after the final fixture-only field-order cleanup |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed; the initial constructor-field-order finding was fixed without changing owner drop order |
| `cargo fmt --all -- --check` | Passed |
| Touched and global `cntryl-tools validate-tests` | Passed |
| Test inventory, documentation, module-size and whitespace validation | Passed |
| `cntryl-tools validate-benchmarks --config .cntryl/repository.toml` | Still blocked by the pre-existing absent `.github/workflows/bench.yml`; no CI configuration changed |

All Cargo lanes use `CARGO_INCREMENTAL=0`. Exact commands, exit codes and wall
times are recorded in `target/fast-tests-final-validation.json`; the original
full-SQL failure set remains retained, with the three resolved identities linked
to the separate passing native-corrected probe. Static inventory is 8,810 Rust
declarations / 8,938 registered instances / 32 targets, with no unregistered,
unresolved, duplicate or oversized files. These counts are not executed outcomes.
The complete repository remains red and the full default suite was not repeated
for this fixture slice. No owner issue is closed.

### Autopilot batch 18: Linear inventory suffix normalization (2026-10-17)

Hosted review of batch17 reported two CodeQL alerts at
`tools/test_inventory.py:89`. The pre-existing `_wave` suffix regex could
repartition digit runs through nested repetitions and hang on small malformed
names. A standard Python unittest regression reproduced a stuck child with two
inputs under 400 characters and a two-second hang guard. It initially failed;
this is a bounded termination check, not a performance timing assertion.

The normalizer now scans underscore-delimited tokens once in reverse. It retains
the earliest valid suffix, ASCII alphanumeric segment rules, Unicode decimal
runs, and the original single-final-newline anchor behavior. No CI configuration,
Rust runtime or existing test-assertion changes. Existing inventory destinations
remain unchanged; the new Python tooling tests have their own `test_inventory`
owner rather than being counted as native PostgreSQL parity coverage.

Finite equivalence checks match the prior implementation for 9,331 short suffix
cases and 17,380 existing inventory names. Standard Python tests cover valid and
invalid grammar, Unicode/newline cases, earliest markers separated by invalid or
empty tokens, and both former hanging inputs. Jev's gap0.33 selected a focused
nested-marker probe; nine explicit nested/invalid-token cases address it.

Ordered validation passes: `cargo build --locked`, seven selected Rust fixture
checks, the three Python unittests (also discoverable with
`python -m unittest discover -s tests/test_inventory -v`, 0.038 seconds), full
workspace/all-target/all-feature strict Clippy, formatting, global Rust test
policy, regenerated inventory, docs, module sizes and whitespace. The benchmark
validator remains blocked by the existing missing workflow; it is not passed.
Commands/results are retained in `target/fast-tests-inventory-validation.json`;
the final Python discovery result is in `target/fast-tests-inventory-discovery.log`. The repository's known
feature failures and the absent benchmark-policy workflow remain separate open
boundaries. Clearing source alerts does not itself establish hosted green.


### Autopilot batch 19: Deterministic scoring cancellation (2026-10-17)

This partial #954 slice replaces the implemented exact-vector and persisted
full-text cancellation races. Each now uses three documents instead of 100,000
and 50,000 respectively. Vector correctness uses explicit in-memory storage;
full-text retains its local persisted index. Neither test sleeps to guess when
scoring has started.

A hidden, request-owned diagnostic pauses once after a scored candidate has
entered the retained top-K state. The test observes that boundary, verifies the
handle was not already cancelled, cancels externally, and resumes execution.
Handle/control clones share the pause; unrelated requests and the other scoring
operator cannot consume it. Disconnection and a ten-second diagnostic hang guard
fail the query rather than leaving a blocked worker. Exact-vector scoring now
checks interruption inside each candidate batch, including its overlay fallback;
persisted full-text checks immediately after retaining a candidate.

The original typed cancellation assertions remain. Both tests additionally prove
zero retained query memory, no running queries or operator workers, no successful
row publication, and same-session recovery with all three results. Focused
regressions cover request/operator isolation, one-shot behavior through cloned
controls, and both disconnected channel endpoints with recovery using the same
handle. The new API was tested red at compilation before implementation; the
original oversized tests passed before replacement (0.93 seconds vector and 0.95
seconds full-text, excluding compilation).

Jev selected request-owned synchronization (0.94 confidence). Adversarial gap
0.61 selected disconnected endpoints; the focused SQL regression exercises both
send failure and receive disconnection, verifies cleanup, and proves the consumed
pause cannot poison reuse. All six aggregate cancellation fixtures remain under
#954 and their pending feature owners #806/#811. This slice does not claim those
features, wire cancellation qualification, or repository-wide green.

Final ordered validation and after timings are recorded in
`target/scoring19-validation.json` and the associated logs. Full vector coverage
passes 172/172 (76.55 seconds); full search remains 64 passing / one known failure
(31.78 seconds). Every prior passing identity is retained. The unchanged search
budget case fails during its setup INSERT at 512 bytes, before scoring; it is not
made green by weakening the cancellation slice. Eight runtime checks pass.

The naming policy requested a narrower name for the request/operator isolation
check. Only that unit-test name changed after the full suite runs; final ordered
build, focused runtime/scoring tests, strict Clippy, formatting and policy checks
are repeated for the final source. The benchmark validator remains blocked by
the existing absent workflow; no CI configuration is changed.


Final gates pass: locked build, 11 focused checks across library/vector/search,
full strict workspace/all-target/all-feature Clippy, formatting, touched/global
test policy, inventory, docs, module sizes and whitespace. Benchmark policy is
still blocked by the missing `.github/workflows/bench.yml`, not passed. The final
Jev review selects the disconnected-endpoint gap as addressed (probability 0.75,
confidence 0.62); executable regressions remain the correctness evidence.

| Cancellation case | Before execution | After execution | Fixture rows |
| --- | --- | --- | --- |
| Exact vector scoring | 0.93s | 0.00s at harness reporting resolution | 100,000 → 3 |
| Persisted full-text scoring | 0.95s | 0.33s | 50,000 → 3 |

These are local developer observations including fixture setup, excluding
compilation, not production performance claims or timing assertions. Final
individual probes launch the already-built binaries; process wall times were
0.011s vector and 0.337s full-text. The whole vector suite retains its other
fixtures and is not claimed faster by this two-case slice. Static inventory is
8,811 Rust integration declarations / 8,939 registered instances; no unregistered,
unresolved, duplicate or oversized files. Static counts are not executed results.


### Autopilot batch 20: Retained string aggregate cancellation (2026-10-19)

This partial #954 slice replaces the 50,000-row `string_agg` cancellation race
with a three-row fixture. A hidden one-shot test pause runs after the aggregate
accumulator has accepted the input and its retained heap charge has been applied.
The test sends a real pgwire CancelRequest at that boundary, waits for the
cancellation connection to close, resumes the aggregate and verifies SQLSTATE
57014, no partial rows, and same-session recovery. The test body completes in
0.59 seconds locally; fixture setup and compilation are not a performance claim.

The pause is captured by the next query cancellation handle on the calling
thread. Unit regressions cover one-shot capture, an unrelated handle, disconnected
observer and resume endpoints, and both guard-drop cases: an unconsumed guard
clears its pending pause, while a captured pause remains attached to its query.
The test hook has a ten-second diagnostic hang guard. The implementation uses
the existing cancellation handle and aggregate retention transition. Aggregate
and scoring diagnostics share one optional test-pause slot so the cancellation
handle's layout does not grow. The change adds no runtime setting or public SQL
behavior.

TDD first showed the focused fixture could not compile before the pause API
existed. Initial integration attempts then exposed two prerequisites: `repeat()`
is unsupported in the string control, and the legacy cancellation helper depends
on unavailable `pg_stat_activity`. The final control uses literal strings and
synchronizes at the retained aggregate-input boundary instead. The unit test for
guard cleanup also caught that a dropped, unused observer reports disconnection
rather than an empty channel; the regression now asserts only that no signal was
received.

Jev selected #954's string aggregate cancellation slice (confidence 1.0), while
its implementation-seam choice had low confidence (0.36). Source inspection led
to query-handle-scoped capture without adding a backend API. In focused review,
Jev assigned 0.68 to the claim that CancelRequest socket EOF follows cancellation
handling, 0.29 to a capture-isolation concern, and selected pause scope for the
first review focus (probability 0.42, confidence 0.27). Source confirms the
CancelRequest handler calls `cancel_pgwire_backend` before returning Break; the
connection loop then exits. The added guard-drop regressions address the selected
scope concern. These are triage signals; the source and passing wire test provide
the evidence.

This is one of the remaining #954 aggregate cases and does not close #954 or #811.
The other collection aggregate fixtures, full repository test baseline, hosted
backend qualification, and workflow-dependent benchmark validation remain
separate work.


### Autopilot batch 21: Downsize pending collection aggregate cancellation fixtures (2026-10-20)

This partial #954 slice replaces the five remaining 50,000-row collection
aggregate cancellation fixtures (`array_agg`, `json_agg`, `jsonb_agg`,
`json_object_agg`, and `jsonb_object_agg`) with three-row inputs. Each test is
wired to the existing query-scoped pause after aggregate input retention and
retains the real pgwire CancelRequest, no-partial-output, and session-recovery
assertions for when its aggregate owner is implemented. Candidate inputs no
longer add `repeat()`, `length()` or `cardinality()` dependencies outside these
aggregate owners.

These aggregate forms remain unsupported under #811 (which still depends on
#804, #807 and #810). On the pre-change `main` run, each test spent about 36
seconds seeding 50,000 rows and then failed its small aggregate control with
SQLSTATE 42601. In the final full run, the changed tests reached their
`count(*)` controls and each failed at the existing five-second retained-input
pause wait (5.64–5.94 seconds per test). The unsupported candidates did not
reach the pause, so no CancelRequest, no-partial-output, or recovery assertion
ran. The test identities remain the same as the baseline, and no active
cancellation coverage is claimed. Their aggregate owners and this cancellation
issue remain open.

The implementation uses the existing `pause_next_query_after_aggregate_input`
test hook already proven by the `string_agg` cancellation slice above. No
runtime behavior, public setting, storage format, CI configuration or workflow
state changes.

At the semantic decision gate, Jev selected the supported `count(*)` control
(probability 0.89, confidence 0.85) over asserting new aggregate serialization
or retaining unrelated scalar wrappers. Source inspection and the focused run
confirm the control succeeds while the candidate still does not reach the
pause; the Jev choice did not convert these pending tests into cancellation
proof.

The focused review asked whether an edited case could pass without reaching
the retained-input pause and which concrete failure mode needed a probe. Jev
assigned 0.75 probability to a false-green path and selected `none` as the first
review concern (confidence 0.39; candidate-error masking had probability
0.36). Source inspection shows the helper panics on a missing pause signal
before it opens the CancelRequest connection, and the exact array case failed
at that wait in 5.75 seconds. Thus the high false-green signal is contradicted
by source and execution; no code change was made. The current unsupported
candidate is reported as a missing-boundary timeout rather than its SQL error,
but the test remains red and names the boundary it failed to reach.

Final local evidence for this slice:

| Command | Result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed. |
| `cargo nextest run --locked --workspace --all-features --no-fail-fast --config-file /tmp/cassie-nextest-60s.toml --status-level fail --final-status-level fail --failure-output never` | 10,000 tests: 8,509 passed, 1,491 failed, 9 skipped. The exact failure-name set matches the saved `main` baseline; no test timed out. The slowest test was 30.60s versus 37.21s at baseline. The command remains nonzero because the existing known-red cases remain. |
| Focused filtered Nextest run for the five edited cancellation tests | 0 passed, 5 failed at the retained-input pause wait in 6.55s each; the real cancellation and recovery assertions remain pending under #811. |
| Strict workspace Clippy with all targets/features and pedantic warnings denied | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f` on the five edited aggregate test files | Passed: 2/2 tests compliant in each file. The touched shared support file contains no tests. |
| `git diff --check` | Passed. |

Repository Actions remained disabled; this slice changed no workflow or CI
configuration and started no workflow. Issue #954 stays open for the remaining
aggregate, vector, and persisted full-text cancellation obligations.

Final local evidence for this slice:

| Command | Result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed after the final handle representation change. |
| `cargo test --locked --lib pause -- --nocapture` | Passed: 8 tests (aggregate/scoring pause regressions and the sibling schema-write pause test), 0.00 seconds of test execution. |
| `cargo test --locked --test pgwire_extended should_cancel_string_agg_without_partial_output_given_retained_values_when_cancel_request_arrives -- --nocapture` | Passed: 1 test, 0.97 seconds of test execution. |
| `cargo test --locked --test pgwire_extended should_reject_string_agg_growth_given_a_memory_budget_when_more_values_arrive -- --nocapture` | Existing adjacent #811 case remains red before its budget candidate: the small control returns 42601 for unsupported `repeat()`. This memory-budget fixture is outside this cancellation slice. |
| `cargo test --locked` | Not completed. The library harness passed 1,231/1,231 in 77.51 seconds; the 179-test analytics harness was interrupted at approximately 59 seconds without a suite summary. Stable libtest does not report individual test durations. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed after aggregate and scoring hooks were combined into one pause slot. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f src/runtime/aggregate_test_pause.rs` | Passed: 4/4 compliant. |
| `cntryl-tools validate-tests -f tests/pgwire_extended/aggregates/string_agg/resources.rs` | Passed: 2/2 compliant. |
| `git diff --check` | Passed. |

The final focused tests remain below the user's 60-second per-test limit. The
77.51-second library figure is the aggregate duration of the 1,231-test harness,
not a recorded individual-test duration; the interrupted full command does not
establish a repository-wide green result.

## Exact parameter OID cache identity (2026-10-17)

Local runtime correction prepared against PR #951 head
`1ca3ddc6ff2236eb5ba8963df91556a545226519`, within the cache isolation
obligations of #778. Shared carrier shapes previously allowed SMALLINT plans
to satisfy INTEGER/BIGINT inputs and BYTEA plans to satisfy TEXT inputs.
Execution-result keys also omitted the effective parameter OID signature.

One private fingerprint owner now includes the complete ordered effective OID
list for Describe, Bind, Execute and result keys. Empty lists preserve the
original fingerprint. Compiled-plan epoch 32 excludes epoch 31 entries without
changing public key structures or serialized schemas. Diagnostics reconstruct
embedded parameter admission; parameterless lookups retain their original
identity. Parameterized `$user` diagnostic lookups conservatively report a miss
because the existing helper signature has no session user.

Behavioral RED on the original runtime produced five failing application tests:
persisted narrow-plan reuse, legacy plan reuse, eligible integer and BYTEA/TEXT
result collisions, and equal keys for reversed OID vectors. Wire probes also
observed incorrect bytes and descriptors while cold-cache controls passed.
Fixture corrections excluded the absent OCTET_LENGTH overload, retained the
actual transaction result-cache bypass, and asserted repeated plan reuse by
increasing hits with zero new misses. They did not change runtime eligibility.

Focused GREEN commands, with `CARGO_INCREMENTAL=0` and
`CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target`:

- `cargo test --locked --lib parameter_cache_tests:: -- --nocapture`: 8 passed.
- `cargo test --locked --test pgwire_extended parameter_cache_identity:: -- --nocapture`: 11 passed.

PostgreSQL 18.6 supplied independent text/binary payload and descriptor witnesses
for integer casts, BYTEA/TEXT concatenation and ordered parameter signatures.
Jev's initial caller gap 0.35 prompted ordered-signature probes. Its later 0.64
gap selected the parameterless `$user` UDF diagnostic probe, which passed after
the empty-input correction; final diagnostic disposition was addressed (1.0).
The final constructor gap 0.54 received the passing 11-test wire probe covering
Describe, Bind and Execute. Independent source review found no additional
confirmed cache defect. These judgments do not establish broader compatibility.

Ordered validation used `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4` and the same
target directory:

| Command | Result |
| --- | --- |
| `cargo build --locked` | Passed |
| `cargo test --locked` | Failed: library 1,094/0; analytics 179/0; backend coverage 2/0; benchmark operations 16/0; catalog/security 241/170. Cargo stopped at that target. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cntryl-tools validate-tests -f <path>` for the five touched test files | Passed |
| `tools/test_inventory.py --check`, documentation, module sizes and whitespace | Passed |

The 170 catalog failure identities exactly match the retained
`target/catalog-datum-final-full-test.log` checkpoint: zero added or resolved.
Raw RED/GREEN records are under `target/cache-identity/`; ordered commands,
exit codes, native frames, Jev receipts and failure comparison are under
`target/finish-line/`. The cache slice does not establish full repository or
hosted green, merge, issue closure or production readiness.

## PR #951 staged merge review closeout (2026-10-17)

This finite checkpoint follows `1ca3ddc6ff2236eb5ba8963df91556a545226519`.
The owner requested merging #951 before a separate full gap analysis. The
checkpoint retains the full #791/#848/#849 program and closes no whole parity
owner. It includes failing acceptance for unfinished or unqualified behavior;
matching failure identities does not classify every retained failure's cause.

Runtime corrections retain the exact ordered effective parameter OIDs in plan
and eligible result-cache identity, resolve parameter BYTEA concatenation and
integer operators after typing, preserve BYTEA comparison/COALESCE identity,
and reject incompatible concrete comparisons. The rebuildable compiled-plan
epoch is 33. No persistent/public key fields or storage layout changed.

BYTEA scratch uses decoded capacity plus operation-specific output, including
unknown-format error buffers. Cast/function paths share hex/escape scanners and
canonical builtin metadata. Runtime integer diagnostics admit exact copied
message capacity before allocation and preserve input detail; native base64
truncation adds its wire HINT. The scoring diagnostic services only its pause;
operator/result publication boundaries own interruption checks.

Behavioral RED preceded the corrections: cache collisions, 18 parameter
operator failures, four BYTEA resource failures, an unknown-format allocation
failure, three diagnostic/hook failures and a 0-versus-57-byte admission
failure. Adversarial probes then caught incompatible concatenation comparisons,
BYTEA cast ordering/control propagation and one full-library constant-planning
regression (1,105 passed / one failed). The classifier now recognizes all six
integer syntax/range formats; its expanded regression and every library test
pass. Three overlong functions were extracted into focused helpers without
lint suppression. Two allocation tests were split without removing assertions.

Final ordered commands used `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4` and
`CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target`:

| Command | Final outcome |
| --- | --- |
| `cargo build --locked` | Passed |
| `cargo test --locked` | Failed: library 1,108/0; analytics 179/0; backend coverage 2/0; benchmark operations 16/0; catalog/security 241/170. Cargo stopped at catalog/security. |
| `cargo test --locked --lib -- parameter_cache_tests:: review_tests:: scoring_test_pause:: qualified_operators:: constant_planning::controls_tests:: should_preserve_native_base64_truncation_hint bytea --nocapture` | Passed: test result: ok. 47 passed; 0 failed; 0 ignored; 0 measured; 1061 filtered out; finished in 0.43s |
| `cargo test --locked --test pgwire_extended -- parameter_cache_identity:: parameter_operator_resolution:: review_diagnostics:: bytea_cast bytea_decode bytea_int2 bytea_int4 bytea_int8 --nocapture` | Passed: test result: ok. 273 passed; 0 failed; 0 ignored; 0 measured; 3488 filtered out; finished in 3.88s |
| `cargo test --locked --test vector_embeddings -- should_cancel_during_exact_vector_scoring should_release_scoring_pause_when_a_diagnostic_endpoint_disconnects --nocapture` | Passed: test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 170 filtered out; finished in 0.01s |
| `cargo test --locked --test search -- should_cancel_persisted_fulltext_scoring_at_a_candidate_boundary --nocapture` | Passed: test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 64 filtered out; finished in 0.37s |
| `cargo test --locked --test sql_queries should_require_finite_regex_execution_before_active_cancellation -- --nocapture` | Known red: 0 passed / one failed; regex execution is absent under #799. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `/Users/jrepanich/Code/cntryl/cassie/target/test-inventory-tools312/bin/python tools/test_inventory.py --check` | Passed |
| `cntryl-tools validate-docs --config .cntryl/repository.toml` | Passed |
| `cntryl-tools validate-benchmarks --config .cntryl/repository.toml` | Blocked: existing missing `.github/workflows/bench.yml`; no CI change. |
| `cntryl-tools check-module-sizes --config .cntryl/repository.toml` | Passed |
| `git diff --check` | Passed |

Touched-test policy commands all passed:

- `cntryl-tools validate-tests -f src/executor/execution/constant_planning_controls_tests.rs`
- `cntryl-tools validate-tests -f src/app/query_bind_tests.rs`
- `cntryl-tools validate-tests -f src/app/query_parameter_cache_tests.rs`
- `cntryl-tools validate-tests -f src/executor/filter/bytea_function_tests.rs`
- `cntryl-tools validate-tests -f src/executor/filter/bytea_resource_tests.rs`
- `cntryl-tools validate-tests -f src/executor/filter/bytea_codecs.rs`
- `cntryl-tools validate-tests -f src/executor/filter/integers.rs`
- `cntryl-tools validate-tests -f src/pgwire/connection/errors.rs`
- `cntryl-tools validate-tests -f src/runtime/scoring_test_pause.rs`
- `cntryl-tools validate-tests -f src/sql/functions/bytea.rs`
- `cntryl-tools validate-tests -f tests/pgwire_extended.rs`
- `cntryl-tools validate-tests -f tests/pgwire_extended/parameters/cache_identity.rs`
- `cntryl-tools validate-tests -f tests/pgwire_extended/parameters/operator_resolution.rs`
- `cntryl-tools validate-tests -f tests/pgwire_extended/types/review_diagnostics.rs`
- `cntryl-tools validate-tests -f tests/support/parameter_cache_identity.rs`
- `cntryl-tools validate-tests -f tests/support/parameter_operators.rs`
- `cntryl-tools validate-tests -f tests/sql_queries/patterns/pattern_matching.rs`
- `cntryl-tools validate-tests -f tests/sql_queries/patterns/pattern_matching/cancellation.rs`

The 170 catalog failure identities exactly match the retained
`target/catalog-datum-final-full-test.log`: zero added or resolved. Later full
integration targets remain unqualified by this default invocation. The focused
wire lane passed 273 tests, including 33 operator regressions, eleven cache
regressions, two diagnostic tests covering seven runtime witnesses, and existing
BYTEA casts/decodes. The focused library lane passed 47; vector cancellation
passed two; persisted full-text cancellation passed one. No failed check was
counted as passed. Compilation and harness durations are retained separately
in `target/merge951/validation.json` and logs. Analytics took 182.23 seconds;
the library took 67.98 seconds and catalog/security 69.60 seconds.

The old `should_cancel_recursive_regex_work_with_cleanup` race (one million
rows and a 25 ms sleep) is replaced with
`should_require_finite_regex_execution_before_active_cancellation`, a three-row
known-red prerequisite. #954 retains actual synchronized cancellation, cleanup,
no partial output and session recovery after #799 implements regex. This is not
active cancellation coverage or a pre-cancelled substitute.

Two review items are explicitly deferred: #816 retains defensive complete
BYTEA-carrier validation until every ingress invariant is proven; #796 owns
qualified-operator token/offset integration and benchmark scaling. Depth probes
pass but do not establish linear parser complexity. Their live owner records
retain the next actions.

Jev's gap judgments drove ordered-OID and cache-diagnostic probes, nested and
mixed parameter operator cases, unknown-format scratch admission, actual wire
C/M/H fields and recovery, exact/below-budget runtime/constant controls and the
six constant-planning diagnostic formats. Confirmed defects were corrected;
final bounded independent source review found no remaining material defect in
the selected changes. Source and executable receipts, rather than probabilities,
establish the finite result.

Native PostgreSQL 18.6 supplied independent OIDs, typmods, text/binary values,
error fields and same-socket recovery. The local oracle executable SHA256 is
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`. Full distribution/corpus and release qualification
remain with #848/#849.

The last audited pre-closeout hosted backend run was cancelled during tests;
CodeQL's aggregate check failed with 143 changed-code alerts. A separate PR-ref
snapshot had 186 open alerts. Specific modeled-flow/test-fixture groups have
source-backed false-positive evidence, but no bulk dismissal or complete
security clearance is claimed. Live main rules were inactive; enforcement, CI,
release and version configuration were not changed. Final pushed-head and
merge readback belong to the PR record. Local receipts are retained under
`target/merge951/` and `target/finish-line/`.

## Issue #955: Ordered quoted-cast input analysis (2026-10-18 UTC)

Runtime repair against merged main `97ebed365986ca111915081d76a25aacebfdd424`.
Explicit quoted casts now reach source/projection binding in native order.
Parameter inference returns a private, nonpublishable prefix for a known rejected
cast so an enclosing function signature cannot overtake the input error. App
admission binds that prefix before cache lookup; wire Parse binds before final
admission and prepared-statement publication.

Red evidence: the three unchanged ordering tests failed at the merged baseline.
Additional probes reproduced aggregate arity overtaking a rejected argument,
EXISTS input errors overtaken by later columns, and lost diagnostic admission
inside a join predicate. Their focused Rust regressions failed before repair.
Refined green evidence: all 27 Parse-analysis tests and six diagnostic/order/
publication guards pass. Full library: **1,114 passed / zero failed**.

Nested known rejected casts reuse the existing binder with current CTE and
correlated-field scope. The existing footprint walker detects rejection while
borrowing the tree; temporary analysis memory is reserved before cloning and
remains leased until the nested bound tree retires. Aggregate arguments bind
before arity checks, followed by FILTER and aggregate ORDER BY. Query-memory
boundary and cancellation probes use small inputs and deterministic controls.
Quoted NUMERIC/DECIMAL retains the existing Float adapter and OID 701; exact
NUMERIC remains under #793/#812.

| Ordered command | Result |
| --- | --- |
| `cargo build --locked` | Passed |
| `cargo test --locked` | Failed at catalog/security: 241 passed / 170 failed; exact baseline failure identities, zero additions |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cntryl-tools validate-tests -f tests/pgwire_extended/protocol/parse_analysis.rs` | Passed |
| `cntryl-tools validate-tests -f src/sql/literal_inputs_tests.rs` | Passed |
| `cntryl-tools validate-tests -f src/sql/parameters/occurrence_tests.rs` | Passed |

Commands used `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4` and the shared target
cache. The first complete gate found a function-length lint and a test-name
policy finding; both were repaired and the ordered sequence was repeated.
The complete remaining-target comparison reconciled **10,102 cases across 33
targets: 7,647 passed / 2,446 failed / 9 ignored / zero incomplete**.
Extended wire is 2,952 passed / 812 failed; its only resolved baseline identities
are the three original #955 regressions, with zero new failures.

The raw full comparison also observed one newly failing upsert waiter and one
newly passing CASE/RETURNING witness. The upsert case fails on clean merged main
at the initial isolated run and all three repeats (and 3/3 on the repair), proving
an existing wait-authority gap under #819/#825/#769. Its earlier full-suite pass
does not qualify blocking at the conflict boundary. The RETURNING witness's
baseline failure contains the correct rows in reverse order; its unordered
assertion varies and is retained as a test-qualification follow-up under
#796/#848, without attributing a runtime fix. After this triage, no introduced
failure was identified. The repository remains red on its broader obligations.

Baseline countercheck used a separate clean worktree at
97ebed365986ca111915081d76a25aacebfdd424 and a separate cloned build cache:

    cargo test --locked --test compatibility compatibility::lifecycle::upsert_concurrency::should_insert_the_waiting_row_given_an_upsert_waiter_when_the_owner_rolls_back -- --exact --test-threads=1 --nocapture

Its one registered case failed at the owner-rollback ordering assertion. The
compiler artifact records the baseline worktree source path; independent binary
repeats also failed. Current runtime source hashes remained frozen throughout
the ordered gate and complete inventory; subsequent changes are documentation.

Native witness: PostgreSQL 18.6 Homebrew on aarch64-apple-darwin25.6.0,
server binary SHA-256
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Thirty-five source/projection priority inputs matched in SELECT and SQL PREPARE
(70 cells). Twenty-five additional live-server cells matched native SQLSTATEs
across seven primitive families, CTE shadowing, correlated names and nested
sources. Odd-length BYTEA hex input correctly retains 22023. Full pinned oracle
qualification remains owned by #848.

Jev selected binder control propagation (0.79) and conditional admitted nested
binding (0.98). Its parameter, nested-budget, adapter and parent-function concerns
triggered focused probes; confirmed findings became regressions and repairs.
The later nested-order concern (0.54) triggered the 25-cell native comparison,
which found no additional mismatch. Jev is triage evidence; source, executed
assertions and current GitHub state determine acceptance.

Raw commands, source hashes, red/green logs, native SQL/results, Jev requests/
responses and target comparisons are retained under ignored
`target/error-order/`. Broad PostgreSQL and PL/pgSQL obligations remain tracked
by #791 and their existing subsystem owners.

## 2026-10-18 — UI sanitizer dependency and runtime qualification (#957)

Baseline is merged #956 (`5513831f4fa71074770ff2bb64b50426b0980e3c`).
The production npm audit was red on DOMPurify 3.4.8, with five published
advisories affecting the Monaco dependency chain. A scoped root override to
3.4.17 repairs the installed dependency graph without changing the current
Askr 0.4 / Monaco 0.56 peer contract. The lockfile changes only DOMPurify.

An audit-only repair was insufficient: Monaco imports its own vendored
DOMPurify 3.4.8 module. The first actual production bundle and bundle regression
remained red after the override. A small resolver now redirects only that
installed Monaco caller's sanitizer import to its audited npm dependency.
It is registered in production, dependency optimization and worker graphs.
Removing the optimizer and worker registrations independently reproduced the
unpatched module in those graphs. No dependency files are patched in place.

Five repository regressions inspect the actual resolved module graph and
emitted sanitizer version, preserve an application-owned mirrored path, and
reject a build whose patched dependency cannot resolve. All five pass. Jev's
importer-isolation concern (0.38) triggered the mirrored-path probe; after
executing it, the remaining concern was 0.17. Its missing-dependency concern
was also probed; the subsequent unsafe-fallback estimate was 0.08. These
judgments guide investigation; executed assertions determine acceptance.

UI validation uses Node 26.0.0 and the declared npm 12.0.1 through
`npx --yes npm@12.0.1`. Fresh `npm ci` and `npm audit --omit=dev --json` pass;
the latter reports zero vulnerabilities. Installed and emitted production
sanitizers are 3.4.17. The following npm scripts run from `ui/` in order:
`gen:adapters:check`, `test:unit -- --run`, `type-check`, `lint`, `fmt:check`,
`build`, `test:e2e:mock`, and `test:e2e`. All eight scripts pass: **121 unit tests, 26 mock-browser cases and two
real-Cassie production-browser cases**. After tightening the missing-dependency
assertion to require its resolution error, all 121 unit tests and types/lint/format
checks pass again. Production code is unchanged. The real browser fixture uses
the validated #956 backend with the current UI distribution.

Raw baseline/green audits, dependency graph, red/green module probes, actual
bundle version receipts, Jev requests/responses and command logs are retained
under ignored `target/ui-sanitizer/`. This is a dependency/runtime repair,
not a complete application security assessment or deployed-image claim.

Ordered backend results, with unchanged Rust/build inputs:

| Command | Actual result |
| --- | --- |
| `cargo build --locked` | Passed |
| `cargo test --locked` | Library 1,114/0; analytics 179/0; backend 2/0; benchmark operations 16/0; catalog 241/170, exact baseline failure identities, zero additions/resolutions; later targets not run |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cntryl-tools validate-tests -f ui/tests/monaco-sanitizer-bundle.test.ts` | Exited zero but found no tests: the Rust policy analyzer does not qualify TypeScript; executed Vitest/type/lint/format gates above cover this file |
| `cntryl-tools validate-docs --config .cntryl/repository.toml` | Passed |
| `cntryl-tools validate-benchmarks --config .cntryl/repository.toml` | Existing blocker: missing `.github/workflows/bench.yml`; no CI edit |
| `cntryl-tools check-module-sizes --config .cntryl/repository.toml` | Passed |
| `tools/test_inventory.py --check` using the repository's Python 3.12 environment | Passed |
| `git diff --check` | Passed |

Backend commands used `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4` and the shared
Cargo target cache. The only source changes during that gate were UI test
refinements and documentation; Rust/build inputs remained unchanged. No complete
repository-green or production-readiness claim follows from this bounded slice.

## 2026-10-18 — Constraint namespace/backing checkpoint (#953, unmerged)

The owner confirmed Cassie is predeployment with no users or deployments; no
legacy-data migration policy is required. The selected repair uses existing
canonical relation keys directly for constraint metadata, publication and
cleanup. Public metadata saves validate/backfill newly unique owners through
the existing publisher. No public signature, key framing, layout marker,
journal shape or CI/release/versioning configuration changes.

TDD receipts under ignored `target/constraint-backing/`:

- Initial 12 namespace/direct-write/backfill/restart cases: four passed/eight
  failed, including admitted duplicates and absent canonical metadata. The
  canonicalization repair made all 12 pass.
- Four public metadata-save probes were red on missing existing-owner backing
  or publication over duplicate data. Routing saves through the publisher made
  all 16 pass. Further named-PK, alias-read, cross-schema/database, cleanup and
  empty-call guards reached 23 passing cases.
- A final empty-cleanup probe was red: publishing a new UNIQUE through that API
  returned `Ok("duplicate")`. Its empty branch now delegates to the public
  validating/backfilling save; the publisher's empty branch retains its private
  metadata writer. Existing collection gates are reentrant. Final focused
  checkpoint result is recorded after completion below.

Pre-refinement ordered validation completed: build passed; locked tests passed
library1,114/0 and analytics179/0, then reached catalog242passed/169failed.
There are zero added catalog failures and one resolved existing default-expression
case. Full strict workspace/all-target/all-feature pedantic Clippy, formatting,
touched test policy, docs, module sizes, inventory and whitespace passed.
Benchmark policy retains the existing missing `.github/workflows/bench.yml`
blocker. Source changes during this attempt were a support-module formatting
order and the final red regression/generated inventory; runtime inputs stayed
unchanged until the caller refinement after the attempt completed.

Complete affected suites before that refinement:

| Target | Result | Baseline comparison |
| --- | --- | --- |
| `storage_indexes` | 185 passed / 7 failed / 1 ignored | Six original failures plus the new empty-cleanup red probe |
| `sql_mutations` | 293 passed / 35 failed | Exact original failure identities; zero additions/resolutions |
| `pgwire_extended transaction_errors` | 5 passed / 1 failed | Existing late-unique-conflict RETURNING failure; zero additions |

Commands used `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4` and the shared Cargo
cache. The user requested a stopping point before final integration. The complete
ordered gate and affected-suite comparisons must be rerun on the refined source
before opening/reviewing/merging the #953 PR. #953 remains open; broader
#769/#819/#833/#791 qualification and the application UNIQUE fallback remain.
No repository-green, hosted-green, merge or deployment claim is made here.

Final checkpoint: **24 focused namespace/backing regressions passed, zero
failed**, on the refined caller path. The empty-cleanup red probe now rejects
the duplicate through the public validating/backfilling save. Full final-head
ordered validation and affected-suite comparisons remain pending; the earlier
attempt is retained under `target/constraint-backing/initial-gate/`.

## 2026-10-19 — Final #953 validation on current main

The patch was reapplied to current main `ed61fbb4bc7a8de0b8ed304c4704022bada6c988`.
It retains the owner-approved canonical identity policy and adds no migration,
key-format, layout, public API or CI changes. The final focused suite passes
**25/25**. A prepared-publication recovery unit probe passes **1/1** after a
canonical pending record is replayed during startup; the existing row is
backfilled, the named duplicate is rejected, and the pending record is retired.

Jev review selected two bounded probes. The concurrent public-save/direct-write
case failed on clean main because a duplicate direct write returned `Ok`, and
passes on the patch; it is retained as a barrier-synchronized regression. The
prepared-publication restart probe passes. The final review selected ready
(0.73 probability; focused-probe option 0.23). These judgments guided probes;
the source and executed tests establish the behavior. Requests and responses are
retained under the worktree's ignored `target/jev/` directory.

Ordered validation on the patch: `cargo build --locked` passed; `cargo test
--locked` passed 1,223 library tests and 179 analytics tests before stopping at
`catalog_security` with 279 passed/123 failed. The exact clean-main suite has
the same 279/123 result. The prepared-publication unit probe was added after
that full run and passed separately (1/1); it adds coverage without changing
runtime behavior. Strict workspace/all-target/all-feature pedantic Clippy,
formatting, test-policy, module-size, regenerated inventory and whitespace
checks passed. The inventory reports 8,614 Rust test declarations
and 8,742 registered Rust instances, with no orphan tests, duplicate
registrations, unresolved paths, unsharded targets or oversized files.

Affected-target readback:

| Target | Result | Clean-main comparison |
| --- | --- | --- |
| Focused namespace/backing | 25 passed | Includes the clean-main-red concurrency probe |
| `storage_indexes` | 188 passed / 2 failed / 1 ignored | Both `int4range` failures reproduce on clean main under #905 |
| `sql_mutations` | 303 passed / 14 failed | All 14 failing test identities reproduce on clean main |
| `pgwire_extended transaction_errors` | 5 passed / 1 failed | Same issue-823 RETURNING-order failure on clean main |

Documentation validation reports 74 pre-existing broken-link diagnostics,
with zero new diagnostics against clean main and 499 baseline diagnostics
removed by refreshing generated inventory pages. Benchmark validation remains
blocked because `.github/workflows/bench.yml` is absent; CI changes are outside
this slice's authorization. The failures above are repository baseline gaps,
not issue-953 acceptance failures. PR #971 later merged this patch on
2026-10-19 as `9a2f7c5c16767662ae25f428ce8b2a561226280a`. Issue #953 remains
open on the current GitHub backlog, so this merge does not claim issue closure
or a repository-wide or hosted-green result.

## 2026-10-19 — #780 VERIFY PROJECTION Describe metadata

The focused WIRE-016 slice gives `VERIFY PROJECTION` statement and portal
Describe the same ten-column schema used by execution. The new extended-protocol
owner sends statement Describe, Bind, portal Describe, Execute and Sync for text
and binary portal formats. It checks both RowDescriptions (column names, OIDs,
sizes, modifiers and formats), one ten-field DataRow, one CommandComplete, no
ErrorResponse and final ReadyForQuery `I`. The existing
`should_preserve_execution_owned_projection_verification_output` test remains
the no-Describe control for Simple Query and extended text/binary execution.
The focused test was red before the repair (no RowDescription; expected two)
and green afterward (**1 passed in 0.05s**).

Jev's one focused review request asked whether schema parity or protocol framing
still had a concrete gap and which failure mode to inspect first. It returned
Noul probabilities 0.25 for schema parity and 0.24 for framing; its Choice
selected `none` at 0.23 confidence (probabilities: none 0.42, schema parity
0.31, portal protocol 0.22, unrelated-command scope 0.05). This was a weak
triage signal. Source inspection confirmed execution and Describe call the same
column-definition helper, the match arm is specific to `VerifyProjection`,
and the executed probe checks both formats and final framing. No additional
change was indicated. Request and response are retained in the ignored
worktree `target/jev/issue-780-verify-projection-describe/` directory.

Ordered validation on this worktree:

| Command | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed |
| `cargo test --locked` | 1,231 library tests passed; stopped at `catalog_security` with 279 passed / 123 failed (target aggregate 73.30s). This matches the existing clean-main failure count recorded above; failed identities were not re-compared in this slice. No individual test timeout was observed. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cntryl-tools validate-tests -f tests/pgwire_extended/descriptors/projection_verification.rs` | Passed 1/1 after renaming the test to describe the single requested-format contract |

The repository Actions permission readback was `enabled: false`; no workflow
was enabled or run. This is a partial qualification under #780. Other fixed
command families and post-`SET` `search_path` reporting remain open obligations;
this record does not close #780 or claim repository-green status.

## 2026-10-19 — #780 fixed projection command Describe metadata

The remaining DIFF, COMPARE, PLAN REPAIR and REPAIR projection command families
now share their execution-owned column definitions with statement/portal
Describe. The focused tests send Parse, statement Describe, Bind, portal
Describe, Execute and Sync for text and binary portal formats. They compare
names, PostgreSQL OIDs, sizes, typmods and formats, then check a DataRow of the
expected width, CommandComplete and ReadyForQuery `I`. The DIFF and COMPARE
fixtures each produce a finite output row. PLAN REPAIR starts from a verified
stale row hash. REPAIR runs twice in one connection against two independent
tables; each fixture proves `repairable = true`, then observes `completed` and
post-verification `verified`.

TDD red: all four new owners observed zero RowDescriptions where two were
expected; the focused test body completed in 0.07s. TDD green: all four pass in
0.07s after sharing the result definitions. The retained no-Describe
`should_preserve_execution_owned_projection_verification_output` control passes
in 0.26s.

Jev received one focused request with the exact diff, WIRE-016/WIRE-030
invariants, command inputs, and test outcomes. It estimated a 0.07 probability
of a concrete gap and selected repair-fixture state as the first inspection
focus with 0.31 confidence. Source review confirmed each repair command uses an
independent stale row hash and records a repairable verification. The tests now
assert that precondition and the successful repair/post-verification fields;
the rerun passes. No additional Jev request was needed. Request and response
are retained under the worktree's ignored
`target/jev/issue-780-fixed-command-describe-review/` directory.

Ordered validation on this worktree:

| Command | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed. |
| `cargo test --locked` | 1,231 library tests and 179 analytics tests passed, then `catalog_security` stopped at 279 passed / 123 failed in 72.77s. This matches the previously recorded clean-main count; failing identities were not compared in this slice. The analytics integration target took 98.03s aggregate. The new focused protocol tests took 0.07s. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f tests/pgwire_extended/descriptors/projection_command_families.rs` | Passed 4/4. |

The pgwire integration aggregator `tests/pgwire_extended.rs` contains module
registrations but no test declarations of its own; the validator reported “No
tests found” for that aggregator path. `git diff --check` passed. No workflow
was run.

## 2026-10-19 — #761 empty-side outer-join shape evidence

This qualification-only slice strengthens SQL-019 without changing runtime
behavior. `should_preserve_exact_output_for_empty_right_cte_on_left_join` and
`should_preserve_exact_output_for_empty_left_cte_on_right_join` use empty CTE
sources on each side and check ordered result aliases, PostgreSQL type OIDs,
sizes and typmods, plus exact row values and NULL positions. The empty-right
`LEFT JOIN` covers fallback and vectorized paths with runtime join-counter
deltas. The empty-left `RIGHT JOIN` asserts the supported merge fallback. Both
focused tests passed together in 1.21s.

Before the new assertions, the existing executor empty-source shape test passed
in 0.75s and the pgwire metadata test passed in 0.74s. Those checks covered row
widths and NULL counts; they did not identify the exact value-to-field mapping.
Jev's decision-gate request selected this SQL-019 probe with confidence 1.0.
Its focused review gave a 0.10 concrete-gap probability and selected physical
path as its first inspection concern (0.41 confidence). Source and runtime
counter evidence confirmed the enabled LEFT case increments the vectorized
join count, the disabled LEFT case records merge, and RIGHT records merge
fallback. No further Jev request was needed. Both requests and responses are
retained under ignored worktree paths `target/jev/issue-761-next-slice/` and
`target/jev/issue-761-next-slice-review/`.

Ordered final local validation:

| Command | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed. The cold build took 58.61s; an incremental repeat passed in 0.16s. |
| `cargo test --locked` | 1,231 library tests passed (66.89s aggregate) and 179 analytics tests passed (95.50s aggregate); stopped at `catalog_security` with 279 passed / 123 failed (73.12s aggregate). This matches the previously recorded clean-main failure count; identities were not compared in this slice. The command stops before the executor integration target, so the final two SQL-019 tests were run separately. No per-test 60-second warning was emitted. |
| Focused executor tests | Passed: 2/2 in 1.21s after splitting the test-policy violations. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed on the final test code. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f tests/executor/joins/join_scope_and_shape.rs` | Passed: 6/6. |

`git diff --check` passed. No workflow was enabled or run. The full #761
relational matrix remains open.

## 2026-10-20 — #796 schema-qualified temporal cast metadata

This qualification-only slice adds one extended-protocol witness for the
already supported `pg_catalog.timestamptz(3)`, `pg_catalog.timetz(3)` and
`pg_catalog.numeric(5,2)` cast spellings. It checks inferred parameter OIDs,
statement and portal RowDescriptions (name, OID, size, typmod and format), and
the executed text values. PostgreSQL 18.6 (Homebrew) returned
`2001-02-17 01:38:40+00`, `20:38:40-05` and `12.35` for the same inputs under
UTC. The focused Nextest case passed **1/1 in 0.073s**. Runtime source did not
change; this does not close the remaining #796 obligations.

Jev received one focused review request with the invariant, source excerpts,
exact diff, PostgreSQL result and test output. It returned a 0.22 probability of
a concrete parameter-inference/Describe gap. Its review-focus Choice selected
parameter inference (0.53 probability, 0.37 confidence); descriptor parity was
0.14, value semantics 0.03 and none/unclear 0.30. Source review confirmed
`pg_catalog.` type names are checked against registered built-ins before the
existing parser handles their modifiers, and cast parameter inference uses the
selected target type. The executable test observed Parse parameter OIDs
`[1184, 1266, 1700]`, matching descriptors on both Describe paths, successful
Bind/Execute and the native values. The signal is resolved without a code
change or another Jev request. Request and response are retained under the
ignored `target/jev/issue-796-qualified-temporal-casts-review/` directory.

Ordered local validation:

| Command | Result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed. |
| Full workspace Nextest with `/tmp/cassie-nextest-60s.toml` | 10,001 run; 8,510 passed, 1,491 failed, 9 skipped. Failure count matches the saved main baseline; the added test passed. No timeout was reported; the slowest observed test was 30.408s. The suite took 1,463.218s total. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f tests/pgwire_extended/types/temporal/parameters.rs` | Passed 3/3. |

No GitHub workflow was enabled or dispatched. The broad full-suite failures
remain baseline qualification work and are not attributed to this test and
documentation-only diff.

## 2026-10-20 — #796 canonical `FLOAT(p)` type selection

This bounded runtime slice implements PostgreSQL 18.6's canonical `FLOAT(p)`
selection using Cassie's existing REAL and FLOAT representations. Precision 1
through 24 selects OID 700; 25 through 53 selects OID 701. The precision does
not become a typmod. PostgreSQL 18.6 confirms text values `1.2345679` and
`1.23456789`, inferred parameter OIDs `[700, 701]`, declared column
`atttypmod` -1 for both, and SQLSTATE 22023 for precision 0 or 54. It rejects
`FLOAT(+24)`, `float8(24)` and `pg_catalog.float8(24)` with SQLSTATE 42601.

TDD red: the new precision-boundary test failed on main because `FLOAT(24)` and
`FLOAT(25)` were unsupported. A separate pgwire assertion exposed that Rust
accepts a leading plus in `u32::parse`, so `FLOAT(+24)` was initially accepted
instead of rejected as syntax. TDD green: precision parsing now requires
ASCII digits before numeric conversion; the boundary, declared-column and
extended-protocol tests pass. The parser helper was split after strict Clippy
found `parse_sql_type` at 101 lines; no lint suppression was added.

Jev's decision Choice selected the shared `DataType::parse_sql` path with
probability 1.0, matching casts and declarations in source. Its focused review
gave a 0.22 probability to an invalid-bound DDL SQLSTATE gap and selected alias
scope as the first inspection concern (0.80 probability, 0.73 confidence).
Source confirms the precision branch matches only the canonical `float` base;
the existing bare-alias test and native rejection probes confirm the supported
boundary. Column and cast parsing share the same diagnostic mapping, so no
separate DDL SQLSTATE path was needed. The signed-modifier bug was found by the
local parser/oracle probe and fixed with an executable regression. No
additional Jev request was made. The request and response are retained under
ignored `target/jev/issue-796-float-type-modifiers-decision/` and
`target/jev/issue-796-float-type-modifiers-review/` paths.

Ordered final local validation on the renamed final test tree:

| Command | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed. |
| Full workspace Nextest with `/tmp/cassie-nextest-60s.toml` | 10,003 discovered; 9 skipped; 8,504 passed and 1,490 failed. The unique failure identities exactly match the saved main baseline except `should_map_float_precision_boundaries_to_postgres_type_oids`, which now passes. No new failure identity appeared. No `TIMEOUT` or `TERMINATED` record; the longest logged failing test was 30.394s. |
| Focused FLOAT precision Nextest cases | 4/4 passed before the test-name cleanup; the final renamed pgwire case passed 1/1 in 0.092s. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed after extracting the FLOAT type-selection helper. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f tests/parser_types/grammar/metadata.rs` | Passed 2/2. |
| `cntryl-tools validate-tests -f tests/pgwire_extended/types/real/float_precision.rs` | Passed 1/1. |

The `tests/pgwire_extended.rs` aggregator change is only a module registration;
`cntryl-tools` reported no test declarations in that file. `git diff --check`
passed. The upsert concurrency case failed once in a full run and also failed
with the same assertion when run alone on clean main; it passed in the final
full rerun. It is recorded as existing baseline instability, not attributed to
this slice. GitHub Actions
permissions remain disabled and no workflow was enabled or dispatched. Issue
[#796](https://github.com/cntryl/cassie/issues/796) remains open for its other
type, cast, and expression obligations.

## 2026-10-20 — #796 selected `bpchar` type spellings

This runtime slice accepts `bpchar` and `pg_catalog.bpchar` as the existing
unbounded character type, and sends `bpchar(n)` spellings through the existing
`char(n)` typmod path. The implementation preserves OID 1042, unbounded
typmod -1, bounded typmod `n + 4`, and bare SQL `CHAR` width one. It does not
add a separate type representation. The extended-protocol witness checks
Parse/Describe/Bind/Execute, parameter OIDs, statement and portal row metadata,
and the padded text value for `bpchar(3)`.

TDD red: the parser rejected `bpchar` as unsupported, and the new protocol
witness failed at Parse with SQLSTATE 0A000. The existing combined bpchar/name
cast case also failed for bpchar before the change. TDD green: the two new
parser cases and the pgwire witness pass; the pre-existing combined case now
fails only for the separate `name` type, which remains outside this slice and
open under #796. PostgreSQL 18.6 reports OID 1042 for both aliases, typmod -1
for unbounded `bpchar`, typmod 7 for `bpchar(3)`, and direct padded output
`y  ` for a three-character cast.

Jev's decision Choice selected the minimal `bpchar` alias mapped to the
existing `DataType::Char` representation (confidence 1.0). Its focused review
assigned 0.25 probability to an invariant gap and selected catalog-scope
validation as the first concern (0.48 probability, 0.31 confidence). Source
and executable tests confirmed `pg_catalog.bpchar` uses the registered built-in
guard, `pg_catalog.unknown` remains rejected, and descriptors and values match
the PostgreSQL oracle. No additional Jev request was needed. The request and
response are retained under ignored `target/jev/issue-796-bpchar-decision/`
and `target/jev/issue-796-bpchar-review/` paths.

Ordered final local validation on the renamed final test tree:

| Command | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed. |
| Full workspace Nextest with `/tmp/cassie-nextest-60s.toml` | 10,006 tests across 34 binaries completed: 8,516 passed and 1,490 failed; 9 tests were reported skipped. Failure identities are a subset of the saved clean-main baseline (1,491 failures); no new failure appeared, and `should_map_float_precision_boundaries_to_postgres_type_oids` passed in this run. No timeout or termination; longest test was 47.941s. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f tests/parser_types/grammar/parameters/parameters.rs` | Passed 5/5 after renaming the alias test to remove a validator-reported multi-behavior name. |
| `cntryl-tools validate-tests -f tests/pgwire_extended/types/character/bpchar_casts.rs` | Passed 1/1. |
| `cntryl-tools validate-tests -f tests/pgwire_extended.rs` | No test declarations in the module-registration aggregator, as expected. |

`git diff --check` passed. No workflow was enabled or run. Issue #796 remains
open for the distinct `name` type and the other type, cast, and expression
obligations.

## 2026-10-20 — #826 CREATE TABLE LIKE AST staging

This bounded slice adds an optional `like` clause to the exported
`CreateTableStatement`, with an `IdentifierPath` source and an ordered vector of
`INCLUDING`/`EXCLUDING` actions. The serde default keeps older serialized AST
values without `like` readable. The parser accepts one standalone LIKE table
element and the PostgreSQL 18 option vocabulary; unknown options and mixed or
multiple table elements reject deterministically. Binding rejects parsed LIKE
before target normalization, `IF NOT EXISTS`, and catalog access, so this does
not create an empty or partial target. Full copy semantics remain open under
[#826](https://github.com/cntryl/cassie/issues/826). The option set and ordering
were checked against the [PostgreSQL 18 CREATE TABLE reference](https://www.postgresql.org/docs/18/sql-createtable.html)
and a PostgreSQL 18.6 transaction probe: `INCLUDING IDENTITY EXCLUDING
IDENTITY` produced a target without identity, while the reversed order produced
one with identity; the probe transaction was rolled back.

TDD red: before adding the AST types, the new parser test target failed to
compile because `CreateTableLikeFeature`, `CreateTableLikeOption`, and
`CreateTableStatement.like` did not exist. Before the binder guard, the new
runtime guard test reached the old field parser and returned SQLSTATE `0A000`
for unsupported type `compat_like_guard_source`, rather than the intended
explicit LIKE execution rejection. The issue's existing broad
`should_apply_like_all_with_explicit_exclusions` case still fails with its
clean-main failure identity; that test performs earlier COMMENT/metadata
operations which remain outside this parser-only increment.

TDD green: six focused parser cases pass, including the issue fixture, ordered
options, the complete PostgreSQL option vocabulary, a quoted schema-qualified
source, old-JSON deserialization, unknown-option rejection, and deterministic
rejection of mixed or multiple table elements. Two focused compatibility tests
pass: one confirms a rejected LIKE leaves no target; the other confirms the
guard runs before `IF NOT EXISTS` and preserves an existing target row.

Jev's semantic Choice selected an optional LIKE clause with ordered actions
(confidence 1.0) and parse-then-guard staging (probability 0.98, confidence
0.97). The focused review estimated a 0.25 probability of a concrete invariant
gap and selected quoted source boundaries as its first concern (probability
0.44, confidence 0.30). Inspection confirmed the scanner preserves quoted
spans, and the quoted schema-qualified parser regression passed; no code change
was indicated. No additional Jev request was made. Requests and responses are
retained under ignored `target/jev/issue-826-like-ast-decision/` and
`target/jev/issue-826-like-ast-review/`.

Ordered final local validation on this source tree:

1. `cargo build --locked --bin cassie` passed.
2. The full suite ran in one Nextest invocation with a 60-second per-test
   timeout (`period = 60s`, one termination, zero grace). It completed in
   1,436.609s: 10,014 tests run, 8,522 passed, 1,492 failed, and 9 skipped;
   there were no Nextest `TIMEOUT` or `TERMINATED` results. Compared with the
   saved clean-main failure identity set, two failures appeared and one prior
   failure disappeared. The two identities were
   `should_insert_the_waiting_row_given_an_upsert_waiter_when_the_owner_rolls_back`
   and
   `should_enforce_numeric_digit_limits_given_dead_integer_conversions_when_statements_are_parsed`.
   The numeric-limit case passed in the focused rerun. The upsert rollback case
   failed in a single-test run on both this tree and the untouched `main`
   checkout at the same timing assertion, so it is a current-main concurrency
   failure outside this slice. The prior failure
   `should_map_float_precision_boundaries_to_postgres_type_oids` passed in this
   run. The full-run log is `target/validation/issue-826-nextest-final.log`; the
   two-case rerun is `target/validation/issue-826-nextest-flakes.log`.
3. The focused parser and binder-guard batch passed 8/8 in 5.718s, including
   the final test name. Its log is
   `target/validation/issue-826-focused-final.log`.
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D
   clippy::pedantic` passed.
5. `cargo fmt --all -- --check` passed.
6. `cntryl-tools validate-tests -f
   tests/parser_types/grammar/ddl/create_table_like.rs` passed 6/6, and
   `cntryl-tools validate-tests -f
   tests/compatibility/lifecycle/tables/table_forms/semantics.rs` passed 12/12.
7. `git diff --check` passed.

The clean-main comparison was a targeted run of the upsert rollback test; the
full suite was not run on `main`. No GitHub workflow was enabled or dispatched.

## 2026-10-24 — #826 selected `LIKE INCLUDING ALL` and indexes

At base `31d743f7`, the historical failing compatibility test stopped during
setup at `COMMENT ON COLUMN` with SQLSTATE `0A000`, before exercising `LIKE`.
The fixture was split so this batch tests only #826's selected clone behavior;
column comments remain with #845, and column storage/compression and other
unselected metadata remain explicitly pending. After that scope correction,
the focused `INCLUDING ALL` test provided the implementation red with
`0A000: CREATE TABLE LIKE option INCLUDING ALL is not supported yet`.

The implemented `ALL` union is `DEFAULTS`, supported `CONSTRAINTS`, `IDENTITY`,
and `INDEXES`. Ordered later actions override one feature at a time. Index
copying is restricted to scalar btree indexes; copied indexes and supported
primary/unique constraints receive target-derived names and target-owned
catalog/storage identities. Composite primary keys and unsupported index
forms reject before target publication. The selected catalog/Midge metadata
is committed with the empty cloned collection; no index entries need backfill.
No public API or persistent metadata format changed.

The focused green evidence is:

- `cargo test --locked --test compatibility should_apply_like_all_with_explicit_exclusions -- --nocapture` — passed; checks ordered exclusions, defaults, CHECK, scalar indexes, identity mode, and independent source/target sequences.
- `cargo test --locked --test compatibility should_copy_scalar_indexes_when_like_includes_indexes -- --nocapture` — passed; checks copied `UNIQUE NULLS NOT DISTINCT` and secondary indexes, included payload/key counts, copied `fillfactor` and `deduplicate_items` options, and duplicate key/NULL rejection (`23505`).
- `cargo test --locked --test compatibility should_preserve_composite_unique_key_order_when_like_includes_indexes -- --nocapture` — passed; preserves `(b, a)` key order and rejects duplicate tuples (`23505`).
- `cargo test --locked --test compatibility should_copy_partial_unique_indexes_independently_when_like_includes_indexes -- --nocapture` — passed; copies partial predicates, resolves same-field index-name collisions, and isolates source/target unique reservations.
- `cargo test --locked --test compatibility should_exclude_only_defaults_when_like_includes_all -- --nocapture` — passed; the default is absent while CHECK and NOT NULL remain enforced (`23514`, `23502`).
- `cargo test --locked --test compatibility should_reject_table_like -- --nocapture` — passed, 2 tests; unsupported options reject before target publication and before `IF NOT EXISTS` short-circuiting.
- `cargo test --locked --test compatibility postgresql18_explicit_table_clone_contracts:: -- --nocapture` — passed, 12 tests.
- Identity-specific compatibility tests and the sequence-collision publication unit test — passed, 3 tests total.
- Final readback after helper extraction and moving the focused cases into `like_options.rs`: `cargo test --locked --test compatibility like -- --nocapture` — passed, 19 tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` — passed after extracting typed option state, field cloning, and initial index metadata writes.
- `cargo fmt --all -- --check`, `git diff --check`, and `cntryl-tools validate-tests -f tests/compatibility/lifecycle/tables/table_forms/like_options.rs` — passed; the new module is 5/5 compliant.
- `cntryl-tools validate-tests -f tests/compatibility/lifecycle/tables/table_forms/semantics.rs` — passed, 15/15 compliant after extraction.

Jev's boundary decision selected the scoped #826 slice (0.73 probability,
0.59 confidence). Adversarial review gap estimates of 0.69 and 0.45 prompted
focused probes for composite UNIQUE key order, partial unique-index predicate
and target identity, generated-name collisions, `NULLS NOT DISTINCT`,
default-versus-constraint action precedence, and UNIQUE INCLUDE payload/options.
All probes passed; the post-probe gap probability was 0.20, with no concrete
defect identified. Requests and responses are retained under ignored
`target/jev/like-all-*.json`. The official PostgreSQL 18
reference confirms that `INCLUDING INDEXES` copies primary/unique constraints
and indexes with target-generated names and that later explicit options can
override options selected by `INCLUDING ALL`:
[CREATE TABLE](https://www.postgresql.org/docs/18/sql-createtable.html).
This batch does not claim the full #826 contract or a green repository test
gate. Full build and full test-suite runs were omitted at the user's direction;
the hosted workflow is unavailable while GitHub Actions are disabled.

## 2026-10-20 — #796 parenthesized CAST result followed by postfix cast

This qualification-only slice adds a focused witness for a remaining #796 cast
shape: `(CAST('42' AS TEXT))::INT`. The test compares it with the equivalent
nested `CAST` and checks the selected INT OID 23 and value 42. The parser's
existing postfix path already wraps each top-level `::` target around the
preceding expression; runtime code did not change.

Jev ranked #796 above the currently blocked #846 COPY and #842 index work
(probability 0.85, confidence 0.81). A follow-up Choice selected the
parenthesized CAST result probe first (probability 0.43, confidence 0.24).
Source inspection confirmed the form reaches the existing cast-chain path. The
focused test and a PostgreSQL 18.6 query both returned 42 with integer / OID 23,
so the low-confidence concern identified missing coverage rather than a runtime
defect. Jev was used for prioritization and probe selection; the executed
comparison establishes the behavior. Requests and responses are retained under
ignored `target/jev/cassie-next-slice-priority-20261020/` and
`target/jev/issue-796-cast-chain-gap/`.

Ordered local validation:

1. `cargo build --locked --bin cassie` passed.
2. Focused Nextest batch passed 5/5 across the parenthesized chain witness and
   adjacent #1003/#1004 type-name controls; 393 tests were skipped. The
   `target/nextest-like-60s.toml` profile enforced the 60-second per-test guard;
   total test execution was 0.012s.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D
   clippy::pedantic` passed.
4. `cargo fmt --all -- --check` passed.
5. `cntryl-tools validate-tests -f
   tests/parser_types/grammar/parameters.rs` passed 10/10; `git diff --check`
   passed.

The full suite was not run. No workflow was enabled or dispatched; GitHub
Actions permission remains disabled.

## 2026-10-20 — #796 chained qualified typmod cast

This qualification-only slice adds a witness for the combined form
`'1.234'::pg_catalog.numeric(5,2)::pg_catalog.text`. It compares the postfix
chain with nested `CAST`, checks the numeric typmod's rounded text value
`1.23`, and verifies the TEXT result OID 25. `parse_postfix_expression`
identifies top-level `::` positions and parses each target through
`parse_data_type`; the source path already supported this combined form, so no
runtime code changed.

Jev selected continued #796 work (probability 0.78, confidence 0.71) and
ranked the qualified typmod chain first among the supplied probes (probability
0.57, confidence 0.43). A source/test search found no existing combined-chain
witness. PostgreSQL 18.6 returned `1.23|1.23|text` for the postfix expression,
nested `CAST` control, and result type. Cassie's initial focused execution
passed the new test 1/1; the final related batch passed 6/6 across parser and
extended-protocol targets, with 4,152 tests skipped. The configured Nextest
profile enforced the 60-second per-test limit; aggregate execution time was
0.075 seconds, separate from the 17.56-second test-target compilation.

The focused Jev review estimated a 0.27 false-green probability and selected
the top-level cast boundary as its first review focus (probability 0.63,
confidence 0.45). Source inspection confirmed parentheses keep
`numeric(5,2)` inside the first type target and the following `::` is found
after the closing parenthesis. The PostgreSQL comparison and passing Cassie
test verify the rounded value and TEXT identity; no further probe or runtime
change was indicated.

This is a qualification increment under #796. It does not close the remaining
cast, type-name, literal, operator or collation obligations.

Ordered local validation:

1. `cargo build --locked --bin cassie` passed.
2. The focused Nextest batch below passed 6/6 (4,152 skipped); the configured
   per-test timeout was 60 seconds and test execution took 0.075 seconds.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D
   clippy::pedantic` passed.
4. `cargo fmt --all -- --check` passed.
5. `cntryl-tools validate-tests -f
   tests/parser_types/grammar/parameters.rs` passed 11/11.
6. `git diff --check` passed. `cntryl-tools validate-docs` reported 75 broken
   local links in untouched docs; none referenced the files changed by this
   slice.

```text
cargo nextest run --locked --config-file target/nextest-like-60s.toml --test parser_types --test pgwire_extended -E 'test(should_bind_chained_qualified_type_modifier_casts) | test(should_bind_parenthesized_cast_result_through_postfix_conversion) | test(should_bind_postgres_chained_type_casts) | test(should_preserve_builtin_identity_given_pg_catalog_qualified_casts_when_projected) | test(should_bind_pg_catalog_qualified_temporal_casts_with_native_metadata) | test(should_describe_chained_qualified_array_cast_expression)' --status-level fail --final-status-level fail
```

The full suite was not run. GitHub Actions remained disabled, and no workflow
was enabled or dispatched.

## 2026-10-20 — #796 pending `name` cast rejection

This boundary-only slice adds a parser regression for the unqualified and
`pg_catalog`-qualified cast spellings `'word'::name` and
`'word'::pg_catalog.name`. The selected type profile does not include the
separate PostgreSQL `name` type, so both casts must reject instead of silently
aliasing it to Cassie's text type. `src/types/schema.rs` validates qualified
names against `BUILTIN_TYPES` and its `named_type` lookup returns unsupported
for an absent `name` entry. No runtime behavior or supported type set changed.

Jev selected `reject_only` for the implementation and `casts_only` for test
scope (probabilities 1.0 and 0.94; confidence 1.0 and 0.91). The repository
contract agrees: `docs/feature-support.md` keeps `name` pending under #796, and
the existing schema registry has no `name` entry. PostgreSQL 18.6 accepts both
forms and identifies their result type as `name`; Cassie's profile deliberately
remains narrower. Jev helped choose the regression boundary; the source and
executed tests establish Cassie's behavior.

The focused Jev review estimated a 0.15 probability of a false-green test and
selected the type-resolution path as the first concern (probability 0.86,
confidence 0.81). Inspection confirmed expression casts pass their target to
`parse_data_type`; the unqualified lookup falls through to an unsupported-type
error, while `pg_catalog.name` fails the `BUILTIN_TYPES` membership check. The
regression asserts the `Unsupported` error kind and exact type-specific message
for both inputs, so the selected concern is covered by source and executable
evidence. Review request and response are retained under ignored
`target/jev/name-type-cast-review-20261020/`.

The final related batch passed 3/3 with 397 tests skipped and 0.011 seconds of
test execution. The `target/nextest-like-60s.toml` profile enforced the
60-second per-test limit. Ordered local validation:

1. `cargo build --locked --bin cassie` passed.
2. The focused Nextest batch below passed 3/3; 397 tests were skipped.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D
   clippy::pedantic` passed.
4. `cargo fmt --all -- --check` passed.
5. `cntryl-tools validate-tests -f
   tests/parser_types/grammar/expressions/semantics.rs` passed 36/36, and
   `git diff --check` passed. `cntryl-tools validate-docs` exited 1 on 75
   broken local links in untouched documentation files; it reported no errors
   for either file changed by this slice.

```text
cargo nextest run --locked --config-file target/nextest-like-60s.toml --test parser_types -E 'test(should_reject_unselected_name_type_casts) | test(should_reject_nonexistent_qualified_types_given_builtin_aliases_when_parsing_a_cast) | test(should_bind_schema_qualified_postgres_type_names)' --status-level fail --final-status-level fail
```

The full suite was not run. GitHub Actions remained disabled, and no workflow
was enabled or dispatched.

## 2026-10-20 — Replay commit abort and same-batch retry

TDD red: the new focused regression initially failed to compile because the
`ProjectionReplay` commit pause point was not defined. TDD green: after wiring
that point immediately before the replay transaction's commit, the conflict
case and its adjacent replay recovery controls passed.

The replay gate now has a deterministic test of a real Schema-family commit
abort. The replay worker commits its row batch in the Data family, stages the
event key and checkpoint metadata in one Schema transaction, and pauses at that
transaction's commit. A competing writer commits a change to the same projection
metadata key. Releasing replay produces Midge's typed retryable `WriteConflict`.
The test observes the row while confirming that neither the event key nor the
new checkpoint is visible. Retrying the same batch leaves one logical row and
commits both the event key and checkpoint together. The existing pre-commit
failure test remains distinct; this test exercises failure from the actual
`tx.commit()` call.

Jev's decision request selected `actual_commit_abort` for the roadmap's
“aborted Schema commit” requirement (probability 0.98, confidence 0.97), and
`same_key_competing_writer` as the smallest deterministic seam (probability
0.99, confidence 0.99). Source inspection confirms the replay transaction uses
`AbortOnWriteConflict`, and the executed regression observes
`StorageRetryKind::WriteConflict`; Jev guided the test boundary, while source
and the focused run establish the behavior. Request and response are retained
under ignored `target/jev/replay-gate-20261020/`.

Jev's focused diff review assigned a 0.51 false-green probability and selected
the commit boundary as its first concern (probability 0.85, confidence 0.81).
The required focused probe passed the new conflict test 1/1 in 5.768 seconds.
Source confirms it pauses after both Schema writes and directly before
`tx.commit()`; the test then commits the competing metadata update and observes
the replay transaction's typed `WriteConflict`. This verifies the review lead
against the implementation and executable test.

Ordered local validation on the replay-gate change:

1. `cargo build --locked --bin cassie` passed.
2. The focused Nextest batch below passed 4/4 with 178 unrelated analytics
   tests skipped. `target/nextest-like-60s.toml` enforced the 60-second per-test
   guard; selected-test execution took 6.551 seconds.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D
   clippy::pedantic` passed.
4. `cargo fmt --all -- --check` passed.
5. `cntryl-tools validate-tests -f
   tests/analytics/projections/projection_lifecycle/metadata.rs` passed 6/6;
   `git diff --check` passed.
6. `cntryl-tools validate-docs` reported 75 broken local links in untouched
   documentation files; it reported no errors in either file changed here.

The full suite was not run. GitHub Actions remained disabled; no workflow was
enabled or dispatched. Hosted evidence remains blocked.

```text
cargo nextest run --locked --config-file target/nextest-like-60s.toml --test analytics -E 'test(should_replay_projection_batch_idempotently_with_checkpoint_metadata) | test(should_retry_projection_replay_when_schema_commit_fails_after_data_commit) | test(should_retry_projection_replay_after_schema_commit_conflict) | test(should_keep_projection_replay_checkpoint_after_uncertain_schema_commit)' --status-level fail --final-status-level fail
```

## 2026-10-21 — #823 per-candidate INSERT RETURNING error order

The existing #823 red cases establish a selected, exact behavior: for two
VALUES candidates that collide on a UNIQUE value, PostgreSQL 18.6 evaluates the
first candidate's `RETURNING EXISTS` expression before processing the later
collision. If that expression divides by zero, it reports `22012`; a conflict
that exists before candidate one still reports `23505`. The test controls also
cover distinct candidates with a runtime error, rollback with no partial rows,
and simple, extended-text, and extended-binary protocol modes.

On main `77ea4e2f`, the focused red batch ran the SQL and wire regressions and
failed 0/2: Cassie reported `23505` instead of `22012` for the later-conflict
case. Each test finished in under 0.45 seconds. Source inspection found that
`execute_insert` staged candidates one at a time but deferred all `RETURNING`
projection until after the candidate loop. The statement read owner was already
captured once and shared with the deferred projection.

The implementation projects each successfully staged candidate immediately
when `INSERT ... RETURNING` contains `EXISTS`, using that same captured
statement-start read owner. It retains each projected row under query-memory
accounting and sends the accumulated rows through the existing final
materialization path. A pre-existing conflict is still detected before any
candidate's `RETURNING`; later conflicts are reached only after earlier
candidates' expressions. Other DML paths and non-`EXISTS` INSERT RETURNING keep
their existing projection flow. Every mutation remains in Cassie's existing
session/Data-family transaction; no cross-family transaction is introduced.

Jev's typed decision request selected per-successful-candidate projection with
the fixed statement view (probability 1.0, confidence 1.0) and selected the
exact case as a bounded v0.3 slice without waiting for the broader #800, #769,
#761, or #848 owners (probability 0.99, confidence 0.98). Source and existing
tests confirm those broader SQL and qualification paths are not needed for the
simple fixture. Jev's first review focus was the error-priority controls
(probability 0.60, confidence 0.47); those controls were retained in the wire
test. The answer guided the slice boundary, not correctness evidence.

The final focused Nextest batch passed 40/40 with 5,350 tests skipped in 47.621
seconds aggregate; `target/nextest-like-60s.toml` enforced a 60-second cap per
test. A preliminary broad `test(/returning/)` filter matched unrelated
`set_returning` and MERGE cancellation modules; it stopped after 23 of 83
selected tests, with 18 passing and five failures outside the changed INSERT
path. That overbroad run is not counted as a clean validation result. The
focused batch below narrows the names to the returning helpers, EXISTS owners,
the exact #823 regressions, ordinary INSERT RETURNING forms, skipped conflicts,
multi-row conflict source order, and lazy row-dependent expressions.

```text
cargo nextest run --locked --lib --test relational_qualification --test sql_mutations --test pgwire_extended --filter-expr 'test(/dml_returning_tests/) | test(/exists_returning/) | test(should_evaluate_first_candidate_returning_before_later_unique_conflict) | test(should_evaluate_each_insert_returning_row_before_a_later_unique_conflict) | test(should_return_correlated_scalar_subquery_values_from_update) | test(should_execute_insert_values_with_explicit_columns_returning_columns) | test(should_execute_insert_returning_wildcard) | test(should_execute_insert_returning_scalar_function) | test(should_execute_insert_select_with_returning_rows) | test(should_preserve_skipped_conflict_insert_returning_contract) | test(should_preserve_source_order_given_multi_row_on_conflict_returning) | test(should_keep_row_dependent_returning_expressions_lazy)' --config-file target/nextest-like-60s.toml --no-fail-fast --status-level fail --final-status-level fail --failure-output immediate
```

Jev's focused review request selected `later_candidate_error_priority` as the
first review concern (probability 0.35, confidence 0.19); its Noul concrete-gap
probability was 0.15. I checked the lead against the source and executable
controls: `execute_insert` completes projection before the loop can process the
next source row, while a pre-existing conflict returns from
`execute_insert_source_row` before any RETURNING row is appended. The 40-test
batch passed the later-conflict, pre-existing-conflict, and distinct-runtime-
error controls across all three wire modes, and also passed ordinary INSERT
RETURNING cases. The same `statement_read_controls` reference reaches every
projection; returned row ownership is attached by `ProjectionOutputMemory` and
the accumulated slots stay in `AccountedVec`. The low-confidence Jev result did
not identify a new failing path, so it prompted no code change or extra Jev
request.

After the final code refactor, `cargo build --locked --bin cassie`, the focused
Nextest batch, `cargo clippy --workspace --all-targets --all-features -- -D
warnings -D clippy::pedantic`, `cargo fmt --all -- --check`, `git diff
--check`, and both affected `cntryl-tools validate-tests -f` checks passed. The
workspace docs validator still reports 75 broken links in the unchanged
`docs/testing/owner-assertions.md`, `docs/testing/planned-tests.md`, and
`docs/sql-dialect-profile-history.md`; it reports no errors in the two changed
documentation files. No full test suite or GitHub workflow was run.

The full test suite was not run. GitHub Actions remained disabled; no workflow
was enabled or dispatched.

## 2026-10-26 — Cassie full-suite and selected #777 client readback

Source checkout HEAD: `db028e02b850c05b5294abe28783b2ace799d59e`. The local
working tree also contains parser/client test corrections, removal of four
orphan catalog tests excluded by the 2026-10-25 scope audit, and a generated
test inventory refresh after three test modules were split into focused leaves.
These local changes are not part of PR #1082's remote head. The parser changes
correct two stale expectations; the psql probe uses the configured root
identity and one statement per command. The inventory currently records 8,480
Rust declarations and 8,608 registered instances, with no unregistered tests,
unresolved paths, duplicates, unsharded targets, or oversized Rust files.

The ordered local run passed `cargo build --locked --bin cassie`, then a fresh
`cargo test --locked --no-fail-fast` completed with three failing targets:
`parser_types` 392 passed / 15 failed, `pgwire_extended` 3,484 passed / 284
failed, and `sql_queries` 1,706 passed / 92 failed (391 failures total). Other
targets passed, including library 1,352/0, `catalog_security` 330/0, and
`compatibility` 368/0 with 12 opt-in cases ignored. The exact full output is
retained at `target/batched-runtime/final-full-test-20261026.log`. This run is
not green. The three failures groups include unfinished selected array, JSON,
aggregate, scalar-subquery and relational behavior, plus stale #796 test
expectations whose coverage is not currently selected. No failure was skipped
or weakened to produce this inventory.

The two active #822 advisory-lock wire cases were removed from
`tests/pgwire_extended/coordination/advisory_notify.rs` because the 2026-10-25
scope audit already selected their removal; this only completes that source
reconciliation and does not claim advisory-lock behavior passes. Jev's
focused classification selected this cleanup (probability 0.60; next option
array core 0.36); the source-backed scope audit, not Jev, authorized the edit.
Its request and response are retained under ignored `target/batched-runtime/jev/`.

After removal, the generated inventory records 8,478 Rust declarations and
8,606 registered instances, with zero unregistered tests, unresolved module
paths, repeated registrations, duplicate identities, unsharded targets, or
oversized files. `cntryl-tools validate-docs` currently reports stale links in
the regenerated `planned-tests.md` and older maps; documentation validation is
not green. `cargo build` is green; the full locked test gate is red. Strict
Clippy, formatting and touched-test validation have not yet been rerun against
this complete working-tree state.

The following selected #777 client probes passed on this working tree:

```text
CASSIE_RUN_PSQL_COMPAT=1 cargo test --locked --test compatibility compatibility_matrix::should_validate_psql_read_model_probe_when_enabled -- --ignored --nocapture
CASSIE_RUN_SQLX_COMPAT=1 cargo test --locked --test compatibility compatibility_sqlx::should_validate_sqlx_read_model_probe_when_enabled -- --ignored --nocapture
CASSIE_RUN_DIESEL_COMPAT=1 cargo test --locked --test compatibility compatibility_diesel::should_validate_diesel_read_model_probe_when_enabled -- --ignored --nocapture
```

Each passed 1/1; logs are retained under `target/batched-runtime/issue-777/`.
The default compatibility target and the full `catalog_security` target also
passed in the full run. Prisma and SQLAlchemy probes were not selected for this
bounded #777 subset and were not run. #777 remains open because its complete
catalog/session lifecycle and exact-revision acceptance record has not been
closed out; #780 remains blocked by #777. The live dependency readback for
#842 showed its #792/#793 prerequisites closed. Its selected scalar-index
contract focus, `CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target
cargo test --locked --test compatibility index_contracts:: -- --nocapture`,
passed 47/47, including index lifecycle, catalog, partial/expression/covering
forms, selected modifiers and rebuild controls. The complete
`CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked
--test storage_indexes` target passed 190 tests, 0 failed, 1 ignored; it includes
persisted scalar-index metadata, query equivalence and restart coverage. Logs
for both runs are retained at `target/batched-runtime/issue-842-index-contracts.log`
and `target/batched-runtime/issue-842-storage-indexes.log`. These results do
not close #842's broader acceptance or the full-suite gate. The full suite is
not green. Strict workspace/all-target/all-feature pedantic Clippy, formatting,
and the psql touched-test policy validator passed. GitHub Actions remain
disabled; no workflow was enabled or dispatched.

The first `cntryl-tools validate-docs` after inventory regeneration reduced
the earlier 696 workspace diagnostics to 111. The remaining findings are stale
links in feature-support and historical test maps; those are being reconciled
against current test sources and documented scope. No documentation check pass
is claimed until the validator returns zero diagnostics.

## 2026-10-26 — #800 TABLESAMPLE implementation follow-up

The 2026-10-26 full-suite baseline recorded 369 failures after the per-engine
ANN barrier repair. The two #800 TABLESAMPLE percentage failures were stale
expectations: PostgreSQL reports SQLSTATE 2202H
(`ERRCODE_INVALID_TABLESAMPLE_ARGUMENT`), while the tests expected the
generic invalid-parameter variant. The implementation now returns the
PostgreSQL diagnostic, and those tests assert the native SQLSTATE.

The pgwire cancellation fixture now creates and seeds its table in separate
simple-query requests. This keeps setup independent from multi-command simple
query behavior and leaves the cancellation assertion focused on TABLESAMPLE.
The parser clause was extracted to `query_table_sample.rs` to keep the query
parser below the repository's source-file size limit. Parameter folding gives
the sampling percentage FLOAT4 (OID 700) and repeatable seed FLOAT8 (OID 701)
context, including when the parameter has no supplied OID.

Focused evidence after these changes: the SQL TABLESAMPLE group passed 13/13;
the selected pgwire TABLESAMPLE tests passed, while the broad `sample` filter
also selected two unrelated array sampling failures under #807. The parser
TABLESAMPLE source tests passed 7/7. This follow-up does not complete #800 or
make the full suite green. Full repository validation is deferred until the
next coherent implementation batch is ready.

## 2026-10-21 — #905/#906/#907 active test scope cleanup

The product roadmap keeps unselected range/multirange, XML, and geometry
requests outside v0.4 until their Type Contracts are selected, and directs
tests for out-of-scope capabilities to be removed from active acceptance. This
change removes only the issue-owned active test modules and planned-test rows;
the three feature requests remain open and runtime behavior is unchanged. The
recorded failure inventory attributed 52 failures to range/multirange, 74 to
geometry, and 40 to XML. A separate failing `catalog_security` assertion,
`should_resolve_builtin_range_subtype_in_pg_range`, remains under the broader
#817 catalog owner and is not part of #905's selected range/query tests.

On the cumulative XML branch (including the range and geometry commits), the
ordered local validation was:

1. `CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target CARGO_BUILD_JOBS=4 cargo build --locked --bin cassie` passed.
2. `CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target CARGO_BUILD_JOBS=4 cargo test --locked` compiled the test targets and passed 1,236 library tests, 182 analytics tests, and 2 backend CI tests. It then failed in `catalog_security` with 279 passed / 123 failed; Cargo exited 101. The out-of-scope test removals did not clear this separate catalog batch.
3. `CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target CARGO_BUILD_JOBS=4 cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` passed.
4. `cargo fmt --all -- --check` and `git diff --check` passed.
5. `cntryl-tools generate-inventory --root /Users/jrepanich/Code/cntryl/cassie-worktrees/ranges --output /tmp/cassie-ranges-inventory.md` generated 1,243 source tests, 8,628 integration tests, and 11 benchmarks/stress tests.
6. `cntryl-tools generate-inventory --root /Users/jrepanich/Code/cntryl/cassie-worktrees/geometry --output /tmp/cassie-geometry-inventory.md` generated 1,243 source tests, 8,554 integration tests, and 11 benchmarks/stress tests.
7. `cntryl-tools generate-inventory --root /Users/jrepanich/Code/cntryl/cassie-worktrees/xml --output /tmp/cassie-xml-inventory.md` generated 1,243 source tests, 8,514 integration tests, and 11 benchmarks/stress tests.
8. `/Users/jrepanich/Code/cntryl/cassie/target/test-inventory-tools312/bin/python tools/test_inventory.py --check` passed: 8,514 Rust declarations, 8,642 registrations, 131 planned tests, and no unregistered tests, unresolved paths, duplicate identities, or shard errors.
9. `cntryl-tools validate-tests -f` exited successfully for `tests/pgwire_extended/mod.rs`, `tests/sql_queries/mod.rs`, and `tests/storage_indexes.rs`; each registration root reports no test declarations in that file.
10. `cntryl-tools validate-docs` reports 75 existing broken local links. None refers to the removed range, geometry, or XML test paths; this matches the previously recorded workspace documentation drift.

The focused `CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target
CARGO_BUILD_JOBS=4 cargo test --locked --test storage_indexes` run on the
range-only branch passed 188 tests with 1 ignored and 0 failures. GitHub
Actions remained disabled; no hosted workflow was enabled or dispatched.


## 2026-10-29 — #839 IF EXISTS notices across pgwire protocols

At merged main `3ff17dc1`, the focused raw-wire test
`should_emit_if_exists_notice_during_extended_query_execution` first failed:
PostgreSQL 18.6 emitted one NOTICE for absent `ALTER TABLE IF EXISTS` during
extended-protocol Execute, while Cassie emitted none (0 observed, 1 expected).
A local PostgreSQL 18.6 `libpq` probe using `PQprepare` and `PQexecPrepared`
confirmed the notice timing and message.

A second regression,
`should_not_notice_when_extended_query_alters_a_relation_created_in_transaction`,
then failed because the classifier inspected only the committed catalog and
emitted a false NOTICE for a table created in the active transaction. A
PostgreSQL 18.6 transaction-local `CREATE TABLE` followed by prepared
`ALTER TABLE IF EXISTS` emitted no notices. The classifier now checks the
active session catalog overlay, and both Simple Query and extended-protocol
transaction regressions pass.

The extended execution path reuses the Simple Query missing-relation notice
classifier and writes the NOTICE before the command tag after successful
execution. The raw-wire regressions cover absent and present `DROP TABLE IF
EXISTS` and `ALTER TABLE IF EXISTS`, NoticeResponse fields, SQLSTATE, command
tags, present-object effects, and transaction-local creation. This changes no
public API or persistent layout and completes only this selected #839 behavior;
the issue remains open for other lifecycle forms.

Final local validation on `audit/v03-839-extended-if-exists-notice-20261029`:

- `cargo build --locked --bin cassie` passed.
- `cargo test --locked` passed: 9,840 passed, 0 failed, 12 ignored; doc-tests
  also passed (0 tests).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D
  clippy::pedantic` passed.
- `cargo fmt --all -- --check` passed.
- `cntryl-tools validate-tests -f
  tests/compatibility/lifecycle/ddl/if_exists_notices.rs` passed: 4/4 compliant.
- `cntryl-tools validate-tests -f
  tests/compatibility/lifecycle/ddl/atomic_lifecycle.rs` passed: 10/10 compliant.
- `cntryl-tools validate-docs` validated 126 Markdown documents.
- `target/test-inventory-tools312/bin/python tools/test_inventory.py --check`
  passed with 8,342 Rust test declarations and 8,470 registered instances; no
  unregistered tests, unresolved paths, duplicate identities, shard errors, or
  oversized files were reported.

The nested `cpu_identity` harness reported no benchmark rows inside a passing
Cargo target; this is not benchmark qualification. GitHub Actions remain
disabled, so hosted checks are unavailable and not claimed as passed.

## 2026-10-29 — #842 expression-index key recovery after restart

`should_recover_expression_index_after_committed_rekey` adds a disk-backed
qualification witness for an expression index on `lower(email)`. It creates
the index over populated rows, commits an indexed-key rewrite and membership
changes, restarts, verifies the matching expression query still selects the
hydrated index, and compares its rows with an independent scan. The focused
test passed on branch
`audit/v03-842-expression-index-restart-20261029`, based on merged main
`19edf3a36fc9d2a65187d137370b6f164fcdfb2e`; no runtime behavior changed.
The ordered local gate passed on that branch: build, full locked tests
(9,841 passed, 0 failed, 12 ignored across 35 targets), strict full pedantic
Clippy, formatting, and touched-test validation (7/7). The same full run
initially exposed a timeout in the pgwire cancellation test. The test now uses
an owned listener and a `pg_sleep` entry barrier instead of rebind/startup and
execution sleeps; its two cases pass, with touched-test validation 2/2.
`cntryl-tools validate-docs` validated 126 Markdown documents. GitHub Actions
remain disabled, so hosted checks are unavailable.

## 2026-10-29 — #839 child-side foreign-key rollback

`should_restore_child_foreign_key_after_column_drop_rollback` starts an
explicit transaction, drops the child column that owns a foreign key, and
rolls the transaction back. It verifies the named constraint is restored and
still rejects an orphan insert. The existing
`should_drop_foreign_key_with_the_child_column_it_is_declared_on` separately
verifies the committed drop remains removed after local restart. The new
focused regression passed 1/1 on branch
`audit/v03-839-next-lifecycle-20261029`, based on merged main
`4d8f334bcec23e3833242befbc117a23c613911e`; no runtime behavior changed.

The ordered local gate on this candidate passed: `cargo build --locked --bin
cassie`; `cargo test --locked` (9,842 passed, 0 failed, 12 ignored across 35
targets; doc-tests passed with 0 tests); strict workspace/all-target/all-
feature pedantic Clippy; `cargo fmt --all -- --check`; and
`cntryl-tools validate-tests -f
tests/sql_mutations/constraints/foreign_keys/foreign_key_ddl_lifecycle.rs`
(18/18 compliant). `target/test-inventory-tools312/bin/python
tools/test_inventory.py --check` passed with 8,344 Rust declarations and
8,472 registered instances, with no unregistered tests, unresolved module
paths, duplicate identities, or shard errors. `cntryl-tools validate-docs`
validated 126 Markdown documents, and `git diff --check` passed. This slice
changes test coverage and documentation only; GitHub Actions remain disabled,
so hosted checks are unavailable and not claimed as passed.

## 2026-10-29 — #839 multi-action ALTER restart

`should_recover_multi_action_alter_after_restart` applies one committed
`ALTER TABLE` that converts an existing nullable INT column to
`NUMERIC(20,0)` and adds a TEXT column with a literal default. It captures the
relation and attribute identity, type OID/typmod, default metadata, existing
rows, and backfilled values, then verifies them after reopening the same local
storage directory. The focused test passed 1/1; this is a coverage addition and
changes no runtime behavior.

The ordered local gate passed on branch
`audit/v03-839-multiaction-restart-20261029`, based on merged main
`c390e0f24fd76e2a3e3c1fbf946716f365523cd6`: build; full locked tests (9,843
passed, 0 failed, 12 ignored across 35 targets; doc-tests passed with 0 tests);
strict workspace/all-target/all-feature pedantic Clippy; formatting; and
touched-test validation (11/11 compliant). The generated inventory check
passed with 8,345 Rust declarations and 8,473 registered instances and no
unregistered tests, unresolved module paths, duplicate identities, or shard
errors. `cntryl-tools validate-docs` validated 126 Markdown documents, and
`git diff --check` passed. GitHub Actions remain disabled, so hosted checks are
unavailable and are not claimed as passed.

## 2026-10-29 — #846 idle and partial-frame COPY cancellation

`should_cancel_idle_copy_from_stdin_inside_explicit_transaction` and
`should_cancel_idle_copy_from_stdin_inside_implicit_transaction` start
`COPY FROM STDIN` with no following data frame, send `CancelRequest`, and verify
that Cassie returns SQLSTATE `57014` and `ReadyForQuery` before another COPY
frame arrives. They cover explicit status `E`, implicit status `I`, late
`CopyData`/`CopyDone` discard, rollback, and successful query recovery. Before
the runtime change, the idle-cancel regression failed red at the five-second
response guard while waiting for `ReadyForQuery`.

`should_preserve_frontend_alignment_when_cancelled_after_copy_tag` sends only
the COPY frame tag, waits until the server consumes it, cancels, then completes
the frame and verifies no rows were applied and the next query succeeds. It
failed red on the two-second recovery guard because dropping `read_exact` after
the tag left the stream misaligned; it now passes 1/1.
`should_preserve_frontend_alignment_when_cancelled_after_partial_copy_body`
waits until the raw pgwire server consumes one of two body bytes before
cancelling. With cancellation selected around the entire frame read, it failed
red on the same recovery guard; with the frame-boundary implementation, it
passes 1/1 and verifies `57014`, no ingested rows, late-frame discard, and query
recovery. `should_fail_explicit_transaction_after_partial_copy_body_cancellation`
repeats this boundary in an explicit transaction and verifies `E`, rollback to
`I`, and query recovery; it passed 1/1.

The deterministic
`should_finish_partially_read_copy_length_before_applying_cancellation` and
`should_finish_partially_read_copy_body_before_applying_cancellation` probes
confirm cancellation after the server has consumed only part of the length or
body; both passed 2/2. The COPY handler selects cancellation only while waiting
for a cancel-safe tag read, then consumes the whole frame and applies
cancellation at that boundary. The ready-message priority and buffered-payload
cancellation probes also cover races within implicit and explicit transactions.
The source cancellation group passed **8/8**, including partial length/body,
partial-body explicit transaction rollback, and raw pgwire recovery. The two
idle pgwire cases passed 2/2, the cancellation waiter tests passed 2/2, and the existing
`should_answer_next_query_after_copy_fail_with_trailing_copy_messages`
regression passed 1/1.

The 16 MiB aggregate stream cap is checked across `CopyData` chunks. The
`should_accept_copy_payload_just_below_sixteen_mib_limit`,
`should_accept_copy_payload_at_sixteen_mib_limit`,
`should_reject_copy_payload_one_byte_over_sixteen_mib_limit`, and
`should_reject_copy_payload_length_overflow` probes passed **4/4** using
numeric lengths, without allocating a large fixture. Existing compatibility
tests also cover
[`should_abort_copy_from_stdin_atomically_on_malformed_input`](../../tests/compatibility/lifecycle/copy/from_stdin/errors.rs),
[`should_stage_table_copy_until_explicit_commit`](../../tests/pgwire_extended/copy/transaction_boundaries/semantics.rs),
and
[`should_discard_staged_table_copy_on_explicit_rollback`](../../tests/pgwire_extended/copy/transaction_boundaries/transactions.rs).

Jev's boundary review estimated a **0.32** gap and selected partial length/body
reads, prompting the two deterministic reader probes. Its follow-up estimated
**0.30** and selected raw pgwire partial-body recovery, prompting the failing
then passing end-to-end test. The next review estimated **0.27** and ranked
explicit-transaction partial-body recovery highest; that test passed 1/1. The
follow-up estimated **0.22** and selected no additional probe, so this bounded
slice proceeds to the local gate. The
questions and responses are retained under ignored
`target/jev/issue-846-copy-cancel/`.

The final ordered local gate passed on this worktree:

- `cargo build --locked --bin cassie` passed.
- `cargo test --locked` passed; every unit, integration-test, and doc-test target
  completed with exit code 0.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  passed. Clippy had also exposed a pre-existing `large_enum_variant` in the
  vectorized-join outcome on the base branch. The outcome now stores its result
  separately from a small action enum, avoiding a new heap allocation; the
  query memory reservation remains alive for the same hash-build lifetime.
- `cargo fmt --all -- --check` passed.
- `cntryl-tools validate-tests -f src/runtime/controls.rs` passed **8/8**;
  `cntryl-tools validate-tests -f src/pgwire/connection/copy/cancellation_tests.rs`
  passed **8/8**;
  `cntryl-tools validate-tests -f src/pgwire/connection/copy/payload_limit_tests.rs`
  passed **4/4**; and
  `cntryl-tools validate-tests -f tests/compatibility/lifecycle/copy/pgwire_cancellation.rs`
  passed **2/2**.
- `cntryl-tools validate-docs` validated **126** Markdown documents.
- `tools/test_inventory.py` and `tools/test_inventory.py --check` both passed:
  1,544 Rust files, 8,354 Rust test declarations, no unregistered tests,
  unresolved module paths, duplicate registrations, oversized files, or
  destination conflicts.
- `git diff --check` passed.

A local client smoke used `psql (PostgreSQL) 18.6 (Homebrew)` over the
PostgreSQL wire protocol to Cassie in memory mode at `127.0.0.1:15432`. It
created `copy_release_smoke (id INT, label TEXT)`, imported CSV with a header
and two rows (`Ada Lovelace` and the quoted value `Grace, Hopper`) using
`\copy ... FROM STDIN WITH (FORMAT csv, HEADER true)`, reported `COPY 2`, and
selected both rows in order. A second `\copy` sent one valid integer row
followed by `bad`; psql reported SQLSTATE `22P02`, a count query confirmed zero
rows were applied, and the next `SELECT 1` succeeded on the same connection.
Cassie shut down cleanly after both client runs. This smoke covers `INT` and
`TEXT` CSV values; it does not claim coverage for other type encodings, larger
stream workloads, extended-query COPY, or pg_dump/pg_restore parity.

This evidence qualifies the bounded cancellation and frame-recovery slice;
broader COPY acceptance in #846 remains open. The changes add no SQL behavior,
public API, or persistent format. GitHub Actions remain disabled, so hosted
checks are unavailable and are not claimed as passed.

## 2026-10-29 — #826 concurrent-refresh cancellation cleanup

The deterministic unit regressions cancel `REFRESH MATERIALIZED VIEW
CONCURRENTLY` before activation, inject an activation failure before
publication, cancel after activation metadata is prepared but before the final
interruption check, verify a cancellation arriving immediately after that check
can complete publication, and repeat cancellation with local storage followed
by restart. The first cancellation and activation-failure regressions failed
red because the candidate stayed `Built`. The pre-check cancellation probe
failed red because the refresh returned success and published the candidate.
The implementation checks interruption immediately before metadata
publication, then discards unpublished output/indexes and marks the candidate
failed on any observed activation error.

All five focused unit regressions passed **5/5**, including the documented
cooperative boundary, restored rows, index state, and a successful refresh
after restart. The materialized-view compatibility group passed **23 passed / 0
failed / 2 ignored**; the ignored cases explicitly exercise unsupported
access-method behavior. The ordered exact-tree gate passed on base
`a4e5ec52566b4032c0a1e617e0b95d09c77107af`: `cargo build --locked --bin cassie`;
`cargo test --locked` (**9,853 passed / 0 failed / 12 ignored across 35
targets**, plus successful doc-tests); strict workspace/all-target/all-feature
pedantic Clippy; and `cargo fmt --all -- --check`. The touched-test validator
passed **5/5**; `cntryl-tools validate-docs` validated 126 Markdown documents.
The generated inventory check passed with 8,350 Rust declarations and 8,478
registered instances, with no unregistered tests, unresolved modules, duplicate
identities, shard errors or oversized files. GitHub Actions remain disabled;
hosted checks are unavailable and are not claimed as passed.

Jev's initial adversarial gap probability was **0.43**; red/green probes covered
cancellation and injected pre-publication activation failure. The follow-up was
**0.44**, so a local-storage restart probe was added and passed. Jev's next
estimate was **0.34**; source tracing then motivated the deterministic
pre-publication cancellation probe, which exposed the missing final interruption
check. A follow-up estimate was **0.42**. The ranked boundary probe selected
cancellation after the final check; the new passing test and matching staged-
write checks support treating this as the cooperative publication boundary.
Jev's typed questions and responses are retained under ignored
`target/jev/v03-826-cancel-refresh/`. GitHub Actions remain disabled; hosted
checks are unavailable and are not claimed as passed.

## 2026-10-29 — #842 composite secondary-index restart

`should_recover_composite_index_entries_after_committed_writes_on_restart`
creates a composite secondary index over populated rows, changes indexed keys,
deletes and inserts matching rows, and leaves an original matching row
untouched to exercise index backfill. After reopening local storage, it verifies
the eligible query selects the composite index and compares the rows with an
independent scan after dropping the index. The focused test passed 1/1; this
slice changes coverage and documentation only.

The ordered local gate passed on branch
`audit/v03-842-composite-index-restart-20261029`, based on merged main
`75dd92692549a8dbc884158e98c86823d48c9a12`: build; full locked tests (9,844
passed, 0 failed, 12 ignored across 35 targets; doc-tests passed with 0 tests);
strict workspace/all-target/all-feature pedantic Clippy; formatting; and
touched-test validation (8/8 compliant). The generated inventory check passed
with 8,346 Rust declarations and 8,474 registered instances, with no
unregistered tests, unresolved module paths, duplicate identities, or shard
errors. `cntryl-tools validate-docs` validated 126 Markdown documents, and
`git diff --check` passed. GitHub Actions remain disabled, so hosted checks are
unavailable and are not claimed as passed.

## 2026-10-29 — #777 selected clients refreshed on merged main

The four bounded read-model workflows were rerun on merged main
`904776fe713be79310022bd709d518eade872119`, tree
`081af16f109815675b645e32429e2d6e5228b1b2`. The tree is identical to the
locally qualified #1127 head; this entry records fresh named-client results at
the merged commit. The clients were tokio-postgres 0.7.18, SQLx 0.8.3, Diesel
2.2.6, and psql 16.6 using image digest
`sha256:557fea37a744d5f4c8faab304b0a90858b53ab119735a88c131fd19dab802f36`.
The psql runner used a local wrapper to route the fixture's loopback connection
through Docker Desktop's `host.docker.internal` address.

The secret-free result manifest and wrapper are retained under the ignored
`target/issue-777-current-clients-20261029/` directory. The manifest SHA-256 is
`e1d44160e4a032f7b82e7d43a66b6bd9de8777676077ac515febd8c46ea0c40c`; the
wrapper SHA-256 is
`63953b34186dbc3f133286bdc4eef3ac03abbdcb4046a8d82cd8efd625d88bfd`.

~~~sh
CASSIE_RUN_PSQL_COMPAT=1 CASSIE_PSQL_BIN=/tmp/cassie-psql16 CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked --test compatibility compatibility_matrix::should_validate_psql_read_model_probe_when_enabled -- --ignored --nocapture
CASSIE_RUN_SQLX_COMPAT=1 CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked --test compatibility compatibility_sqlx::should_validate_sqlx_read_model_probe_when_enabled -- --ignored --nocapture
CASSIE_RUN_DIESEL_COMPAT=1 CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked --test compatibility compatibility_diesel::should_validate_diesel_read_model_probe_when_enabled -- --ignored --nocapture
CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked --test compatibility should_preserve_named_tokio_postgres_session_through_error_recovery -- --nocapture
CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked --test compatibility should_infer_virtual_catalog_parameter_types_without_inventing_names -- --nocapture
CARGO_TARGET_DIR=/Users/jrepanich/Code/cntryl/cassie/target cargo test --locked --test catalog_security should_preserve_database_scoped_information_schema_after_drop_restart -- --nocapture
~~~

All six probes passed 1/1. Each named client emitted its expected catalog,
write/read, transaction, duplicate/missing-relation recovery, and same-session
markers. The virtual-catalog test confirmed inferred TEXT and INT4 parameters,
name-collision handling, and the expected `42703`/`42P01` diagnostics. The
catalog restart test confirmed a dropped relation stays absent in its owning
database while same-named metadata in another database remains intact. Diesel's
missing-relation check remains message-based rather than an exact SQLSTATE
assertion.

This refresh covers the selected four-client workflow on the current main tree;
it does not finish #777's remaining catalog/session/view/routine disposition or
certify pgAdmin/DBeaver desktop clients. #777 remains open, and #780's broader
wire-state matrix remains open. GitHub Actions are disabled, so hosted checks
remain unavailable and are not claimed as passed.
