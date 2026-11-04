# Selected regex newline and cancellation evidence

This increment belongs to #799 and the active regex cancellation obligation in
#954. It preserves the existing finite UTF-8 `regex-automata` profile and adds no
persistent format, SQL type identity, production pause setting, or full ARE claim.

PostgreSQL 18.6 (`18.6-1.pgdg13+2`, aarch64 Linux) supplied the semantic controls.
The original implementation returned false for `regexp_like(E'a\nb', 'a.b')`,
counted two dot matches, and replaced the text with `x\nx`; the oracle returns
true, three matches, and `xxx`. The original `n`/`m`/`p` modes also allowed LF in
negated bracket expressions, and quoted mode accepted incompatible flags.

The first regression run executed 78 pattern tests: 74 passed and four failed at
the new default-mode, negated-class, quoted-flag and active-cancellation cases.
A separate six-input director regression failed before correcting `q` literal
prefixes, `***=` whitespace, and `***:` syntax precedence. Both failures retain
the positive controls and previously supported behavior.

The corrected focused command passed 79/79 tests with no ignores in 0.24 seconds,
excluding compilation:

```sh
CARGO_BUILD_JOBS=2 cargo test --locked --test sql_queries pattern_matching -- --nocapture
```

The SQL controls cover LF modes, leading literal `]`, escaped class tokens,
Unicode values, quoted-mode errors/reset precedence, all selected replacement
overloads, NULL/error/parameter cases and the existing output-memory limit.
The internal AST probes preserve nested class precedence, comments/escaping and
72 original-versus-corrected non-LF match outcomes, including CR and Unicode line
separator. A generated-depth probe reproduced a regression: an original 140-level
class pattern was admitted, but its generated wrappers exceeded the backend
parser limit. The original AST still uses the default nesting limit; only the
generated program receives allowance for one bracket node plus one intersection node per original
class, both counted by the backend nesting visitor. Controls retain
140/249/250-level admission and 251-level rejection. Those
internal backend probes do not select new PostgreSQL syntax.

The real pgwire cancellation probe uses a three-row recursive query. It waits
until the target `regexp_like` search completes and reports its actual match
result, then sends CancelRequest and resumes the paused request. It requires
`57014`, no DataRows or CommandComplete, idle ReadyForQuery, released accounted
query memory and a successful retry on the same connection. Result caching is
disabled in this fixture, so the control/retry cannot bypass regex execution.
Unit probes cover true and false completed searches, request/operator isolation,
one-shot clones, guard cleanup and disconnected diagnostic endpoints. The hook
does not run for `regexp_count` or `regexp_replace`, and does not interrupt a
single search or compilation.

Jev `jev-1.14.0` supplied advisory gap probabilities of 0.37 for negated-class
rewriting, 0.69 for quoted-mode precedence, and 0.40 for whether the cancellation
signal proves actual search work. These led to the bounded class matrix, focused replays of pinned
director/flag controls, the generated-depth repair, and the actual-match-result
diagnostic with true/false probes. Request isolation and cleanup judgments were 0.18 and 0.17. Executable
results and source placement determine acceptance; these judgments do not prove
correctness or complete either owner issue.

Two full locked-suite runs passed 9,910 tests with 12 inherited ignores before
the final generated-depth repair. That final increment uses focused semantic,
hook and class tests, strict workspace/all-target/all-feature Clippy, formatting
and touched-test policy. The integrator records the complete ordered gate on the
final combined query bundle and its hosted final-head checks in the completing
pull request. Other regex/ARE/locale/resource and
aggregate/retrieval obligations remain with their live owners.

The next resource increment replaces the meta engine's implicit cache pool with
an immutable, request-owned PikeVM NFA and one explicit mutable search cache.
It keeps the same UTF-8 `regex-automata` syntax and match/capture behavior.
Compilation first admits a conservative envelope for native NFA backing and
finalization directories, with an explicit native NFA size ceiling. A size-limit
failure drops that attempt before doubling the ceiling and rechecking cancellation.
A completed NFA retains a conservative backing reservation; fixed search tables,
one replacement-capture buffer and the maximum epsilon-stack capacity/resize
backing are admitted before constructing the native cache. Clearing the native
stack does not release its reservation. Drop, malformed-pattern failure and
failed admission release their request's leases. Resource failures use `54000`.

These envelopes are qualified against locked `regex-automata` 0.4.18 and the
current Rust allocation strategy. They need requalification when either changes.
The native compiler documents its NFA size limit as approximate and explicitly
excludes auxiliary compiler heap. Parser/HIR preparation, translated-pattern
scratch and Unicode compiler auxiliary workspace remain unresolved resource
obligations under #799. This increment does not claim a total native compiler
heap or process-RSS limit, nor interruption inside a native compile/search call.

Jev selected this bounded admission increment with probability 0.99. Its semantic
probe judgment of 0.52 led to 13,440 additional old-meta versus direct-PikeVM
match/capture controls across LF, anchors, case, expanded syntax, empty matches,
Unicode and ambiguous captures, all passing before implementation. Advisory
NFA/search-bound gap judgments of 0.49/0.45 led to an independent allocator probe:
176 successful native constructor/search cases stayed within the conservative
retained-NFA/search envelopes and released their backing exactly, including a
240-capture cache. Compiler peaks were separately labelled as including the
unqualified auxiliary workspace. Those finite observations do not prove the
remaining compiler-scratch obligation.

The focused pre-change regression confirmed that `regexp_like('x', 'x')` ignored
a one-byte query budget. New boundary, cleanup and request-isolation tests are
written before the deferred final bundle gate. Source and Jev review are interim
evidence; their execution and the complete ordered gate remain pending until the
resource and dynamic-SIMILAR bundle is frozen, following the current user direction.

The dynamic translation selection is recorded in
[the pattern contract](../sql-pattern-contract.md). Its authored tests cover
literal/parameter/column predicates, strict helper NULLs, invalid ESCAPE with a
NULL left input, exact helper output, malformed regex text without premature
compilation, the third quote separator's `2200C`, result descriptors and retry.
A pinned oracle corpus adds leading/escaped closing brackets, nested classes,
quote separators inside classes, disabled escaping and multibyte escape/class
position controls. In particular, the public lexical helper preserves the
oracle's multibyte bracket-position behavior even when the emitted regex is not
accepted by the finite matcher.

The focused implementation review returned advisory gap judgments of 0.46 for
native match/capture equivalence, 0.68 for the search/publication boundary, 0.37
for SIMILAR lexical states, 0.38 for NULL/error precedence, and 0.40 for new test
API/expected-value assumptions. Actions are a durable old-meta/current-engine
match/capture matrix, the existing real post-search CancelRequest replay, the
pinned twelve-case bracket/escape corpus, and direct SQL plus wire NULL/error
controls. Test APIs were checked against current public source; all of these
new or affected executable probes await the frozen bundle gate. Ownership and
parameter-identity judgments were 0.19 and 0.18. No model judgment is a test pass.

The subsequent source batch adds the finite native word/anchor adapter,
quantified pattern matrix and private argument/prepared-text owners. Seventeen
pinned PostgreSQL controls specify advanced/BRE/ERE/quoted word/escape behavior;
ordered ANY/SOME/ALL controls specify negation, NULL/empty identities and the
malformed-later-element short-circuit. New SQL and wire tests are authored for
those behaviors and descriptors. An owned nested-helper test performs a native
match while its argument/text leases remain held, then checks release on drop.
Prepared-text tests cover old/new backing overlap and rejection before growth.

The native compiler preflight currently derives conservative bounds from locked source:
517 immutable Unicode range tables have a largest table of 894 ranges; all Age
ranges total 1,735, and the simple-fold table has 2,938 keys. The conservative
symbolic-class allowance also covers complements and set-operation temporaries.
A forward compiler cache has 10,000 lazy headers; its transition-key clones
correspond to NFA transition backing. A separate header/prefix allowance includes
old/new version-rollover directories. Analysis and native construction have
separate pre-admission lifetimes; the supplied native nesting limit, including
generated class wrappers, is preserved. Simple tight literals keep the smaller
literal envelope.

Jev selected private quantifier calls, private text/argument owners and the
finite assertion adapter; its AST-context choice favored parsed spans at 0.98.
The compiler preflight selection retained a Unicode expansion gap judgment of
0.60. Its response requires the authored property/Age/fold/negation/set/byte-mode/
repetition corpus and independent native allocation calibration in the frozen
bundle's test phase. The calibration uses the actual private survey source with
standalone control stubs and measures native allocation peaks independently;
it has not run. Version-rollover stress remains separate from small correctness
fixtures. None of these deferred checks is represented as passed. Backreference
and lookaround obligations remain open under #799 while their exact contract is
resolved against the current backend and product scope.

The subsequent focused source review retained advisory gaps of 0.35 for nested
output lifetime, 0.38 for assertion comment/quote contexts, 0.53 for generated
nesting, 0.69 for quantified NULL/negation semantics, and 0.49 for Unicode corpus
coverage. The actions are the authored nested-helper live-owner match test,
seven additional pinned comment/bracket/director/BRE/ERE controls, the existing
140/249/250/251 original-depth regression, an eight-row pinned NULL truth table
replayed across all four modes and both quantifiers/negations, and the native
workspace calibration corpus. These executable replays remain deferred. Source
review also added a zero-budget cancellation control before argument directory
allocation. Lower-probability directory, late-compilation, private-call, workspace
lifetime and supplied-nesting questions remain in the retained review record;
they do not replace the required executable gate.

Independent source inspection found that a case-sensitive private-name guard
could disagree with the registry's case-insensitive lookup. Raw helper calls now
reject the resolved FunctionId, and mixed-case controls are authored. The earlier
advisory private-call probability was 0.07; it did not establish correctness or
prevent this source-confirmed repair. Its executable controls remain deferred.
