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
