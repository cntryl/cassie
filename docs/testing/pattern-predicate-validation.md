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
