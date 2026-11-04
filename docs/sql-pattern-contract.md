# Selected pattern translation contract

This contract selects the next #799 dynamic `SIMILAR TO` increment. It uses
PostgreSQL 18.6 as a lexical translation oracle and Cassie's existing finite
UTF-8/Unicode regex backend as the matcher. It does not select full PostgreSQL
ARE, locale-dependent regex behavior or SQL collation tailoring.

`SIMILAR TO` and `NOT SIMILAR TO` accept TEXT literal, parameter and column
patterns and ESCAPE expressions. They lower to existing nested `FunctionCall`
expressions: `regexp_like(value, similar_to_escape(pattern[, escape]))`, with
ordinary boolean negation for NOT. This introduces no persisted AST variant,
row representation or storage-format change. Bound pattern/escape parameters
have TEXT identity; the predicate result is BOOLEAN (OID 16).

The public helper is `similar_to_escape(TEXT) RETURNS TEXT` and
`similar_to_escape(TEXT, TEXT) RETURNS TEXT`, also qualified by `pg_catalog`.
The PostgreSQL catalog identities are OIDs 1987 and 1986, respectively; return
identity is TEXT/OID 25. Both signatures are strict and immutable, with no named
argument identities. One argument uses a backslash escape. Two arguments accept
an empty escape to disable escaping, one Unicode scalar as the escape, or SQL
NULL (which yields NULL). A non-NULL escape longer than one scalar returns
`22025`, message `invalid escape string`, with hint
`Escape string must be empty or one character.` A NULL pattern returns NULL
even with an otherwise invalid escape. This does not add the legacy non-strict
`similar_escape` helper.

The helper returns translated TEXT without compiling or validating that regex:

- Full-string anchors enclose a noncapturing group. `%` becomes `.*`, `_` becomes
  `.`, and ordinary grouping becomes noncapturing. Dot, caret, dollar and a
  non-escape backslash are literal. Existing repetition, alternation and bracket
  syntax passes to the selected regex backend.
- Bracket tracking preserves wildcard characters inside classes, leading `]`,
  negation and nested bracket forms. This is lexical tracking, not regex
  validation; malformed resulting regexes fail when used by the matcher.
- Escaped characters become regex backslash pairs. Outside brackets, the first
  and second escape-double-quote separators use the PostgreSQL substring-part
  group boundaries; a third separator returns `2200C`. These separators do not
  change predicate truth semantics. This contract does not add a SUBSTRING API.
- A final escape character is discarded, matching the oracle. The literal and
  dynamic predicate forms share this translator, correcting the old parser's
  untested divergence for this case and for `?`, bounded repetition and classes.

Arguments use ordinary eager expression evaluation. A NULL left value does not
suppress an invalid non-NULL ESCAPE on the right. Helper NULL strictness remains
separate from that expression-evaluation rule. Named helper invocation is not
selected, because PostgreSQL exposes no helper argument names.

Translation uses two controlled passes, checked output sizing, query-memory
admission before output allocation, and interruption checks during scanning.
The returned expression value follows the executor's existing value ownership;
compiled NFA/search backing uses the request-owned admission contract in
[pattern validation](testing/pattern-predicate-validation.md). Parser/HIR/native
compiler auxiliary scratch remains a pending #799 resource obligation.

This selection is recorded before runtime implementation. Jev chose the public
helper with probability 0.79 and the oracle lexical core with probability 0.82.
These are advisory decisions, verified against the live pinned oracle and the
[PostgreSQL 18.6 implementation](https://github.com/postgres/postgres/blob/REL_18_6/src/backend/utils/adt/regexp.c).
Tests are authored before implementation and executed with the frozen coherent
bundle under the current deferred-compilation instruction. No new match-helper,
ARE backreference/lookaround or locale capability is implied by translating its
text; unimplemented in-scope forms remain pending under #799 and deterministically
fail through the current backend.
