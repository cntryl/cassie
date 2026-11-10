# Unified query and array closeout

This bundle owns #762, #796, #798, #799, #800, #807 and #808 on main
`7874c91f`. It completes their selected finite read-model contracts, rather than
the superseded complete PostgreSQL parity program. Midge 0.5.2 remains the sole
direct storage layer. No engine changes, new storage abstraction, CI changes,
or unsupported Midge guarantees are part of this work.

## Dependency and source reconciliation

The live native dependencies are #807 after #796/#762, then #808 after #807.
The historical prerequisites #761, #794 and #795 are closed; their closure does
not establish unselected parity. Implement the existing selected contracts in
that order within this one branch, then qualify the integrated result once.

| Owner | Selected contract and retained evidence |
| --- | --- |
| #762 | Scalar/row cardinality, correlated scope, IN/ANY/ALL, lazy conditionals, metadata, bounded execution and synchronized cancellation; `tests/pgwire_extended/subqueries/` and `tests/sql_exists_subqueries/` |
| #796 | Checked CAST/:: chains, qualified names/modifiers/array suffixes and finite C/POSIX collation; `docs/type-contract.md`, parser/type and pgwire grammar suites |
| #798 | Scalar/native comparisons plus the approved versioned durable-definition expansion and NULL-aware join keys; `docs/null-safe-predicate-contract.md` |
| #799 | The finite Unicode/regex contract, native and constraint matchers, translation, replacement, interruption and producer admission; `docs/sql-pattern-contract.md` |
| #800 | Typed VALUES/rows/arity and finite logical-block TABLESAMPLE, preserving exact BIGINT and statement snapshots; relation and sampling suites |
| #807 | Registered array identities, six-dimensional shape/lower bounds, constructors/subqueries, subscripts/slices, functions/operators, wire and persisted rows; Type Contract and array suites |
| #808 | Pull-based UNNEST/selected series, lateral sources, aliases, ordinality, early LIMIT and bounded cleanup; `docs/set-returning-sources.md` |

## Selected implementation details

The 2026-11-04 durable null-safe expansion was already user-approved. A focused
Jev decision on 2026-11-05 selected per-record semantic versions (0.99), rather
than a global-marker-only admission rule. New SQL definitions select version 1;
absent/zero is the legacy contract and keeps rejecting actual null-safe
operators. Unknown future versions fail closed. Existing AST variant encodings
and legacy record bytes remain unchanged. Direct metadata callers explicitly
select the new version. Routines remain outside this expansion.

Jev selected extension of the existing simple field/operator/literal CHECK
carrier (0.99), retaining the independent general-CHECK boundary. NULL-safe join
keys use an explicit private flag and NULL bucket (design support 0.78), while
null-omitting indexes keep authoritative fallback. No index-key layout changes.

## Deferred validation plan (recorded before execution)

New regressions are authored before implementation and deliberately unexecuted
during consolidation. No baseline failure is claimed without an observed run.
After source completion, perform focused Jev adversarial review and probe every
gap probability at or above 0.30. Then freeze the bundle and run build, complete
locked tests, strict workspace/all-target/all-feature pedantic Clippy, formatting
and touched-test policy checks in order. Preserve actual failures and distinguish
reused evidence, fresh passes, pending checks and unsupported boundaries.

Raw issue snapshots and Jev requests/responses are retained under ignored
`target/query-array-closeout/`. Neither a model judgment nor existing test
registration establishes acceptance or production readiness.

## Pre-compilation adversarial review

Jev returned default-folding gap 0.41 and SQL-NULL/JSON-null CHECK gap 0.30.
The first led to removing the parser-specific comparison implementation: the
ordinary SQL binder now resolves literal and checked primitive-cast DEFAULT
comparisons before folding to the existing literal carrier. Focused authored
probes retain unknown-string conversion, the explicit BIGINT/FLOAT 2^53 boundary
and invalid BOOLEAN/integer signatures. The second led to new/existing-row
CHECK probes for absent fields, explicit SQL NULL and a JSON null value.
These probes remain unexecuted until the integrated gate. Legacy nested-admission
and future-version judgments were 0.16 and 0.11; retained negative fixtures and
new future-version rejection witnesses remain part of the gate regardless.

## Current-format durable definition boundary (2026-11-10)

The previous minimum-reader publication dependency is superseded by the
[pre-release format and API policy](release-baseline.md#pre-release-format-and-api-policy-2026-11-10).
Cassie is unreleased; older Cassie storage records and readers are not supported
compatibility inputs. Do not gate current definition publication on a v0.2 marker
transition. Keep the current record validation and SQL behavior, and verify
current-build restart/recovery as part of the owning feature bundle.

## Consolidated source review and qualification attempts

The first test compilation failure was repaired by retaining the serialized
index guard test facade. A later complete attempt stopped during integration
test compilation because new external tests called a crate-private diagnostic
method and borrowed rows from temporary results. Those tests now use public
error variants, owned rows and actual pgwire SQLSTATE/error-recovery witnesses.
Neither attempt executed a completed test result group.

Production builds passed before the final targeted corrections. Source review
then found a legacy scalar-index predicate path using the current expression
policy without its record version, and rollup filtering dropping statement
controls. Both are corrected. Focused probes cover legacy index maintenance,
version-one expression indexes, rollup producer admission/retry and lazy cast
admission. No test expectations or resource limits were weakened.

The final focused Jev producer review returned cast-admission gap 0.40 and
rollup-controls gap 0.31. Mandatory probes are authored for compact CHAR/JSONB/
NUMERIC expansion before a result is returned, an unreached wide CHAR branch,
and one-row rollup filtering with a producer one byte above its configured
budget, followed by same-session recovery and name reuse. They must pass the
integrated gate; these scores are retained as leads, not correctness evidence.
Earlier >=0.30 default/CHECK/future-version/legacy-upgrade leads likewise retain
their focused probes in the unified gate.

Fresh local PostgreSQL 18.6 oracle receipts record scalar cardinality, default
coercion and incompatible signatures, SQL NULL versus JSON null CHECK behavior,
duplicate NULL outer joins, patterns, bounds, zipped UNNEST, series and sampling
extremes. Canonical qualified int4/numeric casts agree. Cassie's pre-existing
qualified int/integer aliases are explicit dialect extensions: PostgreSQL
rejects them with 42704. Expected negative oracle cases remain negative cases.
Raw SQL, observations, version/binary identity and Jev records are in the
ignored evidence directory; no unobserved RED result is claimed.


## Observed first executable gate and selected repair

The first executed complete gate passed all 1,800 library tests. It found four
null-safe-suite failures and one pgwire failure. Unknown SQL strings were still
classified as TEXT against primitive numeric operands, and incompatible BOOLEAN/
integer signatures surfaced as 42601. The fresh PostgreSQL oracle requires
unknown-literal coercion and 42883. Jev selected a shared null-safe primitive
signature qualifier (1.0), before the existing Boolean/numeric binder checks.
This keeps constant DEFAULT folding on the same query binder and comparator.
Ordinary comparisons and CHAR/ARRAY/JSON rules keep their existing owners.

The remaining failures were fixture assumptions: a loaded legacy-index probe
must explicitly reset its new SQL-created version-one record to version zero;
the existing-row CHECK probe must use the selected ADD CONSTRAINT name CHECK
route; the failed-fence probe must compare the actual object-ID counter and
absent view rather than unrelated adaptive-feedback schema records. These
corrections preserve their original invariants. All failed logs remain retained.

The speculative numeric-power producer lead is not an admitted SQL path:
function_validation rejects the unselected NUMERIC power overload before
execution (#812). The preliminary Jev direction assumed it was reachable;
source verification disproved that premise. This bundle does not enable it.


## Completed local qualification

The complete locked test run passed **10,385 / 0 failed / 12 existing ignored**
in 426.88 seconds, including compilation on this warm local workspace. This is
local evidence, not a prediction of hosted cold-run timing. Strict Clippy then
required two focused helper extractions and style fixes. After extraction,
all 1,800 library tests and 3,169 affected integration tests passed (one existing
ignored integration case). Test policy split independent cast/legacy/transaction
scenarios and clarified the coupled lifecycle invariant names, retaining every
assertion. The final four evaluator probes and 978 definition/pgwire tests pass.
No new test is ignored and no resource, error, NULL or multiplicity assertion is
removed.

| Check | Observed outcome |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed; repeated after helper extraction |
| `cargo test --locked --no-fail-fast` | 10,385 passed, zero failed, 12 existing ignored |
| `cargo test --locked -p cassie --lib` after extraction | 1,800 passed |
| Affected `executor`, `metrics`, `null_safe_predicates`, `pgwire_extended`, `sql_queries`, `storage_indexes` suites | 3,169 passed, one existing ignored |
| Final evaluator and definition/pgwire organization checks | 4 and 978 passed respectively |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed, including every test and benchmark target |
| `cargo fmt --all -- --check` | Passed |
| `cntryl-tools validate-tests -f <path>` | All 91 touched Rust paths passed |
| `tools/test_inventory.py --check` under the existing Python 3.12 tooling environment | Passed; every integration declaration registered |
| Qualified-operator parser diagnostic | All five sizes passed |

The parser diagnostic used `CASSIE_STORAGE_MODE=memory cargo bench --locked
--profile test --bench tier2_subsystem_parser -- --workload
sql_parser_qualified_operators`. It preserves the declared fixed-work fixture
and samples, using the optimized test build profile. Observed rates for 8, 16,
32, 64 and 128 operators were approximately 4,930, 2,490, 1,240, 630 and 297
statements/second. This is a filtered diagnostic; it does not establish a release
performance baseline or prove linear parser complexity. It supplies the missing
finite scaling observation for #796 without putting timing assertions in tests.

The unified PR's Validation and Status sections own live final-head hosted
outcomes and merge readback. GitHub Actions remain enabled. The independent
#1188 engine-wide/native-background WAL fence is still unavailable through the
published API and remains open. Completing these seven selected contracts does
not complete the v0.2.0 milestone or production readiness.


## Review correction before merge

PR #1192 was returned to draft when a new numeric CHECK probe reproduced a
CheckViolation for BIGINT 7 against literal 7.0. Raw JSON equality was not the
selected SQL comparison law. Jev selected reuse of the binder/evaluator (1.0).
Version-one non-NULL simple CHECKs now retain a strictly validated bound
comparison, with ordinary query coercion and metadata heap/identity accounting.
The numeric, unknown-literal, JSON-null, prepared-shape, unprepared-admission,
request-memory and restart probes pass in the focused run. Review leads were
comparison law 0.76, shape admission 0.14 and ownership 0.30; mandatory focused
probes remain part of the complete gate. No new general CHECK syntax is selected.

A separate escaped-JSON resource probe reproduced output serialization before
sufficient admission. The scalar workspace bound now includes exact serialized
JSON bytes as well as retained document backing; the probe uses both denial and
admitted controls. The previous head's backend checks passed, while CodeQL flagged
a generic result debug print in a synthetic resource test. The debug value was
removed, retaining the same resource assertion. These corrections require fresh
local and final-head hosted qualification; earlier checks are superseded.


## Corrected bundle qualification

The corrected complete `cargo test --locked --no-fail-fast` run passed
**10,395 tests, zero failed, 12 existing ignored**. The escaped-JSON regression
was observed RED before its serialized-workspace correction and is GREEN in
this complete run. All four numeric CHECK oracle cases also pass in isolated
PostgreSQL 18.6 and in the Cassie regression; prepared CHECK restart recovery
passes. The three subsequent Clippy style corrections were validated by 22
affected library tests and all 65 null-safe integration tests; the final empty-
record assertion change passed its exact integration witness.

The final strict workspace/all-target/all-feature pedantic Clippy and formatting
checks passed. Test policy passed on all **100** Rust paths touched by the unified
PR. Static inventory registration and whitespace checks passed. No new ignore,
reduced lint level, removed correctness assertion, or CI/dependency/version
configuration change was introduced. The PR's current head must receive fresh
hosted qualification; the earlier head's successful backend run and flagged
CodeQL debug print remain superseded evidence.
