# Backfill and organization validation

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
original analytics failures and the new vector/JSON admission probe pass. The
four remaining SQL failures are string_agg ordering, grouping sets, ordered-set
rank aggregates and statistical aggregates; they remain pending obligations.
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
