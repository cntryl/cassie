# Backfill and organization validation

Recorded on 2026-10-15 on `feature/postgres-18-compat`, from checkpoint `00000118` (which preserved the existing dirty backfill). One branch and one worktree were used. Runtime source, Cargo configuration, corpus expectations and CI configuration are unchanged. This is a test-only change; no runtime fixes or issue closures are claimed.

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
