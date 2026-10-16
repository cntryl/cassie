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
