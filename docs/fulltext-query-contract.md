# Full-text query contract

This document is the contract for the ranked full-text query surface of
`search()`, `search_score()` and `snippet()`. It records the grammar, the
limits and the diagnostics before any execution code relies on them. The
implementation lives in `src/search/query/` and is shared by the authoritative
row evaluator and every indexed path, so the two cannot disagree on meaning.

## Syntax selector

`search`, `search_score` and `snippet` take an optional third argument that
selects how the query text is interpreted.

```sql
SELECT _id, search_score(body, $1, 'query') AS score
FROM articles
WHERE search(body, $1, 'query')
ORDER BY score DESC
LIMIT 10;
```

| Third argument | Meaning |
| --- | --- |
| omitted | `'terms'` |
| `'terms'` | Existing behavior, unchanged: the query is analyzed into tokens and a row matches when any token is present, scored by BM25. A two-argument call and a `'terms'` call are byte-for-byte the same. |
| `'query'` | The query grammar below. |
| anything else (including `NULL`) | Invalid-parameter error, SQLSTATE `22023`. |

The argument is a string literal or a bound parameter. The query text itself
may also be a bound parameter. `hybrid_score(search_score(body, $1, 'query'),
vector_score(embedding, $2))` is supported; the hybrid score is computed by the
authoritative row evaluator when the text side uses the `'query'` syntax.

## Grammar

```
query      := or_expr
or_expr    := and_expr ( "OR" and_expr )*
and_expr   := unary ( [ "AND" ] unary )*
unary      := ( "-" | "NOT" ) unary_item | "+" primary | primary
unary_item := primary
primary    := "(" or_expr ")" | phrase | word
phrase     := '"' chars '"' [ "~" digits ]
word       := chars [ "*" | "~" [ "1" ] ]
```

Precedence, tightest first: `NOT` / `-`, then `AND` (adjacency), then `OR`.

- Adjacent items are conjunctive: `red shoes` needs both words. An uppercase
  `AND` between items is accepted and means the same thing.
- `OR` (uppercase keyword) gives alternatives: `red OR blue`.
- `-item` or `NOT item` excludes rows that satisfy `item`. `-` must be directly
  attached to the item it negates (`-shoes`, `-"red shoes"`, `-(a b)`);
  `NOT` is the uppercase keyword followed by whitespace or `(`. A
  doubly-negated item (`--a`, `-NOT a`, `NOT -a`) is invalid.
- A leading `+` is accepted as an explicit "required" marker and is the same as
  no marker.
- Parentheses group.
- Lowercase `or`, `and` and `not` are ordinary words (and, being stop words
  under the default analyzer, are dropped like any other stop word).
- A `-` or `+` that is not directly attached to an item (a lone `-`, or `a - b`)
  is an ordinary word that analyzes to nothing and is dropped.
- Items are separated by whitespace; `(`, `)` and `"` also end a word.

### Quoting and escaping

- A backslash escapes the next character, inside and outside quotes: `\"`,
  `\\`, `\(`, `\-`, `\*`, `\~`, `\ ` are literal characters of the word or
  phrase text. An escaped operator character loses its operator meaning.
- A backslash as the last character, an unterminated quote, unbalanced
  parentheses, `()`, a dangling `OR`, `AND` or `NOT`, and an `OR` or `AND` with an
  empty side are invalid-parameter errors.

### Phrases and proximity

`"red running shoes"` matches only when the analyzed terms occur in that order
at consecutive token positions of the same document.

- Positions preserve gaps. They count every token the tokenizer produced,
  including stop words the analyzer removes, so `"alpha beta"` does not match
  the text `alpha the beta`, and `"alpha the beta"` matches it. Stop words at
  the start or end of a phrase are trimmed; interior ones keep their gap.
- `"a b"~N` allows up to `N` extra positions in total between the terms. The
  terms stay ordered: `"a b"~2` matches `a x y b` but never `b a`.
  `N` is an integer from 0 to 8; `"a b"~0` is the exact phrase.
- A word that analyzes to several tokens (for example `alpha-beta` under the
  standard tokenizer) is an implicit phrase with slop 0. A word that analyzes
  to a single token is just that term. A word or phrase that analyzes to no
  token (a stop word such as `the`) is neutral and is dropped from the query.
- A phrase never degrades to unordered matching. On the indexed path the order
  is verified from the token positions persisted per document.

### Prefix and typo terms

- `word*` matches every index term that starts with the (folded) word.
  The prefix must be at least 2 characters.
- `word~` (or `word~1`) matches every index term whose Levenshtein distance
  (insertions, deletions and substitutions of one character each; a
  transposition costs two) to the word is at most 1, the term itself included.
  The word must be at least 4 characters. `~2` and larger are invalid.
- Prefix and typo words go through case folding and accent folding only, in the
  way the index analyzer is configured. They are **not** stemmed, are not
  checked against stop words and do not use synonyms: they are compared with the
  stored, already analyzed index terms. With `stemming = english` the index
  stores `run` for `running`, so `runn*` matches nothing; query `run*`.
- A prefix or typo word must be a single token. A word whose folded text
  contains a token boundary (`foo-bar*`) is an invalid-parameter error.
- A prefix or typo item may be negated, grouped and combined like any word.
  Quoted phrases do not take `*` or `~` suffixes.

## Matching rules

A row matches when the Boolean expression is satisfied:

- A word, prefix or typo item is satisfied when the document contains at least
  one of its terms.
- A phrase item is satisfied when it has an ordered occurrence as above.
- `AND` needs all operands, `OR` needs any operand, `NOT` needs the operand
  unsatisfied.

Every matching row must contain at least one positive item. This is checked
syntactically, before stop words are dropped:

- a conjunction needs at least one non-negated operand (`-a` and `-a -b` are
  invalid; `a -b` is fine), and
- every alternative of an `OR` needs to be positive (`a OR -b` is invalid).

Violations are invalid-parameter errors. They keep every matching set
reachable from the postings of the positive terms.

Degenerate queries are defined, not errors: an empty or blank query matches
nothing (as for `'terms'`), and a query whose positive items all analyze to
nothing (`the`, `the -cat`) matches nothing.
A parenthesized group whose positive items all analyze to nothing is dropped
from its parent like a stop word, together with its exclusions.

## Scoring

A row's score is the sum of the BM25 contributions of the distinct index terms
that belong to satisfied positive items:

- for words, the term; for prefix and typo items, every expanded term the
  document contains; for phrases, the phrase's terms, counted only when the
  phrase matched;
- items under `NOT` contribute nothing;
- items in unsatisfied `OR` branches contribute nothing;
- term frequency is the term's total occurrences in the document and
  BM25 statistics (document count, document frequency, average length) are
  corpus-wide, exactly as for the `'terms'` syntax. The boost, `k1` and `b`
  index options apply as before.

`search(..., 'query')` is true when the expression is satisfied.

## Snippets

`snippet(body, q, 'query')` wraps in `<mark>` the source spans of the tokens
that satisfied positive items. For phrases only tokens that take part in a
phrase occurrence are marked, so a stray occurrence of one phrase word elsewhere
in the text is not. Items under `NOT` are never marked. A row that does not
match is returned unmarked.

## Limits and diagnostics

| Limit | Value |
| --- | --- |
| Query text | 1,024 bytes |
| Items (words, phrases, prefix and typo terms) | 32 |
| Nesting depth (parentheses, negations) | 8 |
| Phrase span, in token positions | 16 |
| Phrase slop | 8 |
| Minimum prefix length | 2 characters |
| Minimum typo word length | 4 characters |
| Expanded terms per prefix or typo item | 128 |
| Expanded terms per query | 512 |

The limits are named constants in `src/search/query/limits.rs`.

| Condition | Error | SQLSTATE |
| --- | --- | --- |
| Unknown syntax selector, bad escape, unterminated quote, unbalanced parentheses, empty group, dangling operator, no positive item, nested negation, slop or `~N` out of range, prefix or typo word too short or not a single token, query text, item count, depth or phrase span over the limit | invalid parameter value | `22023` |
| A prefix or typo item expands to more than 128 index terms, or the query expands to more than 512 terms in total | program limit exceeded, naming the limit | `54000` |
| Query memory budget exceeded | resource limit, as for any query | `54000` |

Expansion limits are never applied by silently truncating the term list: the
statement fails. The failure happens before scoring, leaves the session usable
and the same statement can be retried (for example with a longer prefix).
Indexed and authoritative execution expand against the same vocabulary — the
index terms of the field — and fail with the same error for the same query.

The expanded term lists, the parsed query and the per-item matching state are
accounted against the per-query memory budget
(`QueryExecutionControls::reserve_query_memory`).

## Execution

- The authoritative evaluator (`search_context` row evaluation) analyzes each
  row's text with positions and evaluates the query directly.
- The indexed path expands prefix and typo items against the index vocabulary,
  reads the posting blocks of the positive terms, reads each candidate's
  persisted statistics record, which carries per-term token positions and
  evaluates the same query on those positions. It never fetches the source row
  to verify a phrase and never degrades a phrase or prefix to unordered token
  matching. Posting blocks hold no positions.
- Inside an open transaction that touched the collection, the indexed path is
  not used and the authoritative evaluator runs over the transaction's view.
