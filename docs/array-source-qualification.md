# Native array and generated source qualification

This records the finite #807/#808 implementation bundle against the selected
[Type Contract](type-contract.md) and [source contract](set-returning-sources.md).
Midge remains the published `cntryl-midge` 0.5.1 dependency. No engine source,
storage layout or public value representation changes belong to this bundle.

## Existing prerequisites

The foundation already implements #796 casts and #762 scalar/row/IN/ANY/ALL
subqueries. Before this bundle's runtime work, the isolated foundation passed
36 cast cases, 16 lexical/control cases, and 35 scalar/quantified subquery cases.
The array identity prerequisite commit `45c15ae4` passed 219 existing array
pgwire cases. These focused results are prerequisite evidence, not complete
milestone or PostgreSQL compatibility qualification.

## Implementation and witnesses

| Requirement | Implementation | Focused witnesses |
| --- | --- | --- |
| Native literals, rectangular shape, six dimensions, NULL/empty distinction | `types/array_value.rs`, `sql/binder/source_bindings.rs`, scalar array evaluator | `types/arrays/constructor_contracts.rs` |
| Array result/parameter identity, bounds and concatenation | Array binder summaries, concrete input coercion, existing subscript/predicate kernels | Existing array pgwire suites plus constructor and UNNEST source cases |
| UNNEST row-major scalar output, longest-input NULL padding, aliases and ordinality | Private native-array cursor | `set_returning/unnest.rs`, `types/arrays/unnest_sources.rs` |
| Five finite series families and native input/output identities | Shared finite signature resolver, parameter fold and checked cursor state | `set_returning/series.rs`, pgwire `set_returning/series_contracts.rs` |
| Ordinary filtered LIMIT, aliases, derived sources and lateral joins pull lazily | Private source cursor tree and existing controlled collection transport | `source_pull_tests.rs` exact advancement/decoded-row counters |
| Cancellation and retained row ownership | Existing query controls, per-source/per-row owners, controlled expression scratch | Synchronous row-boundary cancellation for UNNEST and all series families; direct collection-row lease lifetime witness |
| Native array statement formatting and canonical JSONB | Captured private result session context; existing native formatters | Temporal arrays under connection TimeZone; canonical JSONB text/binary cells |
| Admission before native text and frame scratch allocation | Allocation-free typed render and native array preflight bounds | Native NUMERIC scale, bit magnitude, rank-six escaped nested JSONB and frame capacity witnesses |

Test paths in the table are scoped modules under `tests/sql_queries`,
`tests/pgwire_extended`, or source-local execution tests. They assert values,
NULLs, result OIDs, widths, typmods, text/binary formats, inferred/supplied OIDs,
SQLSTATEs, terminal no-partial-output and same-session recovery.

The benchmark owner `tier2_subsystem_generated_sources` compares the same
filtered LIMIT operation with 2,048 and 100,000 logical integer source rows.
It records query/output counts through the existing stress artifact format.
No correctness test uses elapsed-time or throughput assertions.

## Evidence and remaining gate

Historical executed RED evidence includes eager UNNEST advancing five rows for
a two-row filtered consumer, missing native source OIDs, ambiguous input and
NULL-fill cases, and unadmitted computed `array_fill` output. Small executable
loops were then deferred by user direction until this substantial bundle froze.
New regressions were authored first; source, pinned PostgreSQL18.6 receipts and
bounded Jev review guided implementation during that interval.

The complete frozen build/test/clippy/fmt/policy gate has exposed integration
defects, described below. Strict lint, final formatting/policy checks and the
release benchmark artifact remain pending. Runtime support and issue closure
are not claimed by this record while the gate is incomplete.
The first build found five compiler errors; those were corrected. Subsequent
builds passed, with the latest clean build taking 53.15 seconds. Integration
compilation found an existing AST destructuring fixture and a new wire setup
shape requiring updates; both were corrected without weakening assertions.
The initial PostgreSQL batch passed 22 of 23 expectations: the remaining
expectation used an unquoted BIGINT minimum whose cast fails before unary
negation. The fixture now quotes the minimum; the original failed receipt is
retained. The corrected PostgreSQL18.6 batch passed 34/34, including
native JSONB array text/binary framing and volatile projection/OFFSET behavior.

Source review also caught OFFSET being applied before projection and projected
row expansion overrunning LIMIT in the private cursor. Pagination now counts
projected output rows, matching the existing execution pipeline. Focused
sequence witnesses distinguish skipped projection invocations from merely
returned values, including inner and outer derived pagination.

Jev's original source gaps (consumer 0.43, identity 0.59, advancement 0.59,
admission 0.69) and follow-up findings (computed parameter 0.32, numeric format
0.31, array producers 0.70) remain recorded. The final implementation review
returned identity 0.31, ownership 0.33 and advancement 0.30; each maps to focused
final-gate witnesses above. Native frame review additionally assigned 0.36 to
frame admission, 0.39 to numeric/bit magnitude and 0.48 to nested JSON bounds;
the corresponding frame-capacity regressions are authored. Lower later scores
do not waive any original probe. Jev selected reusing the existing controlled
collection cursor (1.00) and canonical JSONB array formatting (0.99).

Raw requests, responses, pinned receipts, historical red logs and ordered gate
logs remain under the isolated worktree's ignored `target/v020-evidence/`.
They contain no API credentials. Finite local evidence never establishes full
PostgreSQL parity, hosted CI, merge completion or production readiness.


## Complete integration pass and correction checkpoint

The first complete no-fail-fast pass executed all 35 groups: **9,928 passed,
18 failed, 12 ignored**. The failures were confined to native table-source
parameter folding/descriptors, existing plain local/ancestor/DML CTE binding,
VALUES array admission, unary source arguments and duplicate default labels.
The same pass kept all 1,428 source-local unit tests green, including source
stop counters, cancellation, native frame bounds and row owner lifetimes.

A batched correction now visits selected source function children in the
parameter fold, derives source column types through the existing typed shape
API, limits coercion traversal to relevant array/source expressions, preserves
unqualified generated scalar labels during alias allocation, and reuses native
scalar argument binding. VALUES admits native array casts/constructors and
retains controlled expression memory until its row takes ownership. Array text
casts admit parsing scratch first. Array-valued subquery validation checks
inputs in order, preserving native NULL/shape/rank error precedence.

The correction checkpoint built cleanly. The expanded pinned PostgreSQL18.6 semantic batch passed
**39/39**, including ordered array-subquery failures and unary INT4 overflow.
Jev correction gaps remain recorded: parameter fold 0.39, CTE scope 0.36,
VALUES ownership 0.38, and ordered array-subquery candidate 0.66. The existing
18 regressions and newly authored array-parser entry/budget and ordered-error
witnesses are required probes in the complete gate. This checkpoint does not
claim issue closure, strict-lint success or benchmark qualification.

## Final source correction batch

The subsequent complete gate at `0a8e82a2` passed its build and executed all
35 test groups: **9,938 passed, 9 failed, 12 ignored**. No strict-lint, final
formatting or policy result is implied by that failed gate. All nine failures
remain as regressions. They identify generated result-schema fallback, array
constructor and visible alias type inference, early unbound source signature
diagnostics, and the existing pending character-array modifier cast boundary.

The final source-only correction adds generated source metadata through the
existing typed source schema, routes function projections through native array
constructor inference, and adds alias keys only to expression lookup. Renamed
VALUES aliases precede conflicting bare physical names while private-qualified
references remain intact. A separate witness swaps `column1`/`column2` aliases
over array and TEXT values. Single collection aliases retain their existing
physical lookup precedence.

The parameter fold defers non-parameter generated source validation to typed
binding. Scalar/ARRAY subquery result summaries reuse its admitted projection
targets and source contexts rather than a new catalog traversal. Added wire
witnesses cover inferred/supplied series parameter OIDs, correlated scalar
bounds, and a parameter inside an ARRAY subquery passed to UNNEST. Ambiguity,
unsupported concrete signatures and the original nine cases remain required
integrated probes.

Pinned PostgreSQL18.6 source establishes ordinary UNNEST result typmods of -1.
The implementation erases input modifiers only from the scalar descriptor;
four wire witnesses retain VARCHAR, CHAR, NUMERIC and TIME input values.
The prior 39/39 semantic receipt remains valid for its frozen SQL batch; these
new modifier, alias and parameter witnesses are authored and unexecuted.

Jev assigned the descriptor gap 0.86 and selected base scalar OID descriptors
(0.90). It assigned alias collision 0.33 and early fold deferral 0.82, selecting
reuse of admitted subquery summaries (1.00). Those gaps require the focused
integrated probes above; no later judgment waives them. Jev selected restoring
the declared pending modified character-array VALUES cast boundary (1.00).
The unchanged existing resource assertion must continue to receive `0A000`;
the unimplemented element conversion remains a #796/#807 obligation.
The final source review selected integration (0.91) while retaining subquery
forwarding 0.36 and alias lookup 0.44 as mandatory focused probes. The authored
parameter/subquery and swapped alias regressions provide those probes in the
integrated gate. Signature diagnostics scored 0.23 and modifier values 0.17;
those judgments are not executable validation.

Per the user's deferred-run instruction, this final correction batch has no
new compilation or test result. Rustfmt parsed/formatted the touched Rust
modules; this is source formatting evidence only. The integrator will run one complete ordered
gate on the assembled substantial bundle, then collect the release benchmark
artifact. Full #807/#808 closure remains pending that evidence and the explicit
remaining cast obligation.

The subsequent [character-array cast increment](character-array-cast-qualification.md)
implements that staged conversion boundary as a separate source-only change.
Its new regressions and full qualification remain pending; the checkpoint
counts above describe their exact earlier source revisions.
