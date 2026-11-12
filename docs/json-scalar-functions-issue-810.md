# JSON scalar functions (#810)

This closeout note records the finite #810 boundary and its current executable
evidence. [Feature Support](feature-support.md) remains the behavior matrix;
[Type Contract](type-contract.md) remains the selected type and SQL/JSON
contract.

## Scope and dependencies

The live #810 acceptance covers JSON inspection, construction and conversion,
`json_object_keys`, finite `json[b]_extract_path[_text]` functions, JSONB
mutation helpers, and the selected SQL/JSON forms. That SQL/JSON subset is
`JSON(input WITH/WITHOUT UNIQUE KEYS)`, ordered-query `JSON_ARRAY`, and
`JSON_EXISTS(... ON ERROR)`. `JSONPath` expression operators/functions are the
separate #809 increment in the Type Contract; JSON table functions and
`JSON_TABLE` belong to #829. The two `jsonb_path_query_first_tz` probes currently
tagged #810 in `docs/testing/inventory.json` therefore need issue-attribution
reconciliation; they are not a reason to expand this slice.

The live prerequisites #809 and #793 are closed. Current main contains the
native JSON/JSONB identities and finite #809 operators required by this slice.
No pre-release compatibility or migration between unreleased builds is part of
the contract.

## Corrected JSON_EXISTS default

PostgreSQL 18.6 returns FALSE when JSON path evaluation errors and `ON ERROR` is
omitted. The active Type Contract previously said the default was ERROR, and
`json_exists_policy` supplied that value to the evaluator. The corrected
contract defaults to FALSE; explicit FALSE, TRUE, UNKNOWN and ERROR policies
retain their existing behavior. PostgreSQL's [JSON_EXISTS reference](https://www.postgresql.org/docs/18/functions-json.html#FUNCTIONS-SQLJSON-QUERY) states that omitted `ON ERROR` returns FALSE.

Pinned local oracle (`PostgreSQL 18.6`, Homebrew, aarch64-apple-darwin25.6.0):

```sql
SELECT JSON_EXISTS('{"a":1}'::jsonb, 'strict $.a[*]'),
       JSON_EXISTS('{}'::jsonb, 'strict $.missing');
```

Observed output: `f|f`. The first operand triggers a strict structural path
error; the second is a missing-path control. The authored regression
`should_default_json_exists_structural_errors_to_false` asserts both BOOLEAN
results over pgwire. Existing
`should_apply_json_exists_error_policies_to_strict_structural_errors` covers
the explicit policies, SQL NULL for UNKNOWN, SQLSTATE `22039` for ERROR, and
connection recovery.

The focused PostgreSQL 18.6 error-boundary oracle is recorded in
`target/v020-evidence/810/postgresql18.6-json-exists-error-boundaries.txt`.
Structural path evaluation errors return FALSE under `FALSE ON ERROR`; invalid
path syntax and malformed context-item JSON still raise errors before the
policy applies. This matches PostgreSQL's distinction between path evaluation
errors and context-item conversion errors.

Jev selected the PostgreSQL 18.6 FALSE default (0.98 probability, 0.97
confidence) and the finite documented #810 boundary (0.97 probability, 0.96
confidence). The source-backed choice and response artifacts are retained under
the ignored `target/issue-810-review/` directory. These judgments do not replace
the oracle or the Cassie regression.

## Explicit variadic path arrays

The live acceptance includes finite path helpers. The supported helper family
therefore includes explicit `VARIADIC text[]` calls for all four
`json[b]_extract_path[_text]` functions, in addition to positional TEXT
components. PostgreSQL 18.6's `pg_proc` reports the path argument as `text[]`
with `provariadic = text`; an unmarked `text[]` argument is not the same call
form and fails overload resolution.

Pinned PostgreSQL 18.6 cases are recorded in
`target/issue-810-review/postgresql18.6-json-variadic.sql` and `.out`:

- A normal text array traverses the path. A multidimensional `ARRAY` constructor
  is consumed in row-major element order. Cassie's SQL type parser rejects
  declared `text[][]` type syntax, so the regression exercises the constructor
  directly; dimensions are carried by the runtime array value.
- An empty array returns the input document. A NULL array or NULL path element
  returns SQL NULL.
- JSON-valued extraction preserves document null; `_text` extraction returns
  SQL NULL for document null. Missing paths return SQL NULL.
- A `VARIADIC $1` path parameter is inferred as TEXT[] (OID 1009).
- Passing the same text array without `VARIADIC`, or passing a non-TEXT array,
  deterministically fails overload resolution with SQLSTATE 42883.

Observed excerpts: `null||` for JSON document null, text extraction of that
null, and a missing path; `{"a": 1}|` for an empty array and a NULL array;
`3802 1009|text|jsonb` for `proargtypes`, `provariadic`, and `prorettype`;
`1|` for a two-dimensional path array; `null|null|` for JSON/JSONB document
null and text extraction; and PostgreSQL's function-does-not-exist errors for
unmarked `text[]` and `integer[]`.

Jev selected `include_variadic_text_array` (0.92 probability, 0.88 confidence)
from the live issue, current finite path contract, and oracle evidence. Focused
pgwire regressions now cover values, NULL distinctions, parameter inference and
the unsupported ordinary-array boundary. The array form uses the same bounded
path traversal and output-owner lifecycle as positional components; focused
resource, cancellation and owner tests cover those paths. Cassie's ordered
gate will record whether the authored probes pass.

A focused Jev review asked which acceptance seam needed another probe. It chose
`no_extra_probe` at 0.39 probability and 0.24 confidence; its largest specific
gap was parser/binder parameter flow at 0.28. The action is to retain the
authored `VARIADIC $1` descriptor-and-execution regression and verify it in the
ordered gate; Jev supplied no evidence for weakening the accepted syntax.
Request and response artifacts are under the ignored review directory.

## Current acceptance status

The source implements the bounded inspection, construction, conversion,
mutation, finite path, and selected SQL/JSON forms described above. The #810
change corrects the omitted `JSON_EXISTS ON ERROR` default and implements the
explicit variadic path-array form. The Type Contract records the corrected
JSON_EXISTS default; the coordinator owns the shared Feature Support matrix.
`JSON_EXISTS` maps JSONPath evaluation errors through its selected policy while
preserving query control, context-item input, and path-compilation failures.
The source-level
`should_preserve_json_exists_control_failures_given_false_on_error` regression
covers resource denial and cancellation, and
`should_propagate_json_exists_input_errors_given_false_on_error` covers invalid
context JSON and path syntax. Both are authored and pending the coordinator's
ordered gate. The multidimensional variadic regression uses the supported
`ARRAY[['a']]` constructor because Cassie's parser rejects declared `TEXT[][]`
type syntax.

The focused Jev adversarial review identified one source-level admission gap:
`JSON_EXISTS` copied an owned JSON context value and path string into a temporary
argument array before query-memory admission. The implementation now passes
borrowed operands to the shared JSONPath evaluator and reserves its estimate
before parsing or cloning the JSON context. `serialized_json_bytes` counts
serialized size through a counting writer, so the admission estimate does not
build a second serialized buffer. The new
`should_admit_owned_json_exists_context_before_applying_false_on_error`
regression uses an already-owned, large JSON context with a deliberately
insufficient query budget and asserts resource-limit propagation plus released
accounting. It is authored but unexecuted pending the coordinator's bundle
gates. Source review confirms the call site no longer deep-clones the context
or path before admission; the regression exercises the resource-denial boundary
but does not independently measure allocator activity.

The focused Jev review asked, “Which implementation contract best matches the
PostgreSQL 18.6 oracle and active Cassie Type Contract for `JSON_EXISTS ON
ERROR`?” Jev selected `apply_only_to_eval_errors` (confidence 1.0), matching
the tagged evaluator-error boundary. It also asked for the most concrete
remaining #810 closeout obligation; Jev selected the coordinator-owned Feature
Support matrix (probability 0.57, confidence 0.43), with inventory attribution
at 0.25 and another specific gap at 0.14. The initial review's
`other_specific_gap` probability of 0.32 prompted the PostgreSQL 18.6 error
boundary probe above. The action is to keep shared support-matrix edits with
the coordinator and retain the #809/#810 testing-inventory attribution note for
coordinator reconciliation; the final review found no further source gap.
Request and response artifacts are under
`target/v020-evidence/810/jev-adversarial-review-final-20261111.json` and
`target/v020-evidence/810/jev-adversarial-response-final-20261111.json`.

The exact branch head and ordered local-gate outcomes for this bundle are
reported at handoff. #810 remains open until the coordinator completes
integrated delivery and reads back the live acceptance state; this note does
not close the issue.
