# JSON path-deletion unknown literals

Status: proposed source contract; tests are authored and unexecuted. Existing
`json_delete_path_family` accepts a NULL/parameter or known TEXT[] RHS, but
not an unknown `Expr::StringLiteral`. `binder/select.rs` runs
`boolean_contexts::validate_select` before operand-family validation;
`resolve_integer_operator` invokes `json_extract::qualify`, which already
coerces unknown `#>`/`#>>` literals but is a no-op for `#-`. No earlier
`json_predicates` rewrite handles `#-`.

PostgreSQL 18.6 evaluates
`'{"a":{"b":1,"c":2}}'::jsonb #- '{a,b}'` as `{"a": {"c": 2}}`,
and both unknown operands in `'{"a":1}' #- '{a}'` as `{}`. NULL yields SQL
NULL. An explicitly typed TEXT RHS returns 42883, an invalid array literal
returns 22P02, and a multidimensional array returns 2202E. Cassie's existing
JSON extension is a separate selected contract and must not be removed.

A bounded proposed fix selects PostgreSQL's JSONB/TEXT[] overload by wrapping
only unknown `Expr::StringLiteral` operands in the same existing scalar Cast
AST used for JSON extraction: an unknown left as JSONB and an unknown right as
TEXT[]. Already typed JSON/JSONB, explicitly typed TEXT, NULL, parameters and
native arrays retain their existing paths. The normal left-then-right expression
evaluation, cast diagnostics, NULL handling, native-array multidimensional
rejection and JSONB output type remain observable; evaluate each once. This
literal-only binding does not establish an equivalent rewrite in unbound user
function bodies. Existing array cast construction/admission, JSONB canonicalizer,
sentinel representation and general expression ownership remain unqualified.
The selected unknown-left choice and its oracle evidence are recorded below.

Binder-level and pgwire tests cover valid nested/empty paths, both unknown
operands, NULL, explicit TEXT, malformed literal and multidimensional rejection.
A PG18.6 oracle established the listed results; no Cassie build, tests or
clippy have run. No issue-completion or full JSONB migration claim is made.

Jev selected both direct unknown literal coercions (0.90; RHS-only 0.06;
semantic/priority lead 0.77), receipt
`target/assembly-review/json-delete-unknown-literals-{request,response}.json`.
A PG18.6 oracle also found invalid JSON beats a malformed path when both are
literal, while NULL JSONB plus malformed path reports 22P02; NULL path with a
valid document yields SQL NULL. Authored Parse/Bind/Execute, bound TEXT[],
explicit TEXT, JSON extension, malformed/dimension, control and user-routine
witnesses remain unexecuted. Existing parser/cast physical capacity, cancellation
and already-transformed sentinel data remain separate obligations.

Wire Parse timing remains a distinct open gap. The parameter Fold runs before
these binder-generated casts, and current scalar literal validation has no
TEXT[] input rule. Unlike PostgreSQL's Parse-time 22P02 for an invalid unknown
array literal, this selected source change can defer that error to later cast
or constant evaluation. The successful ParseComplete/descriptor witness does
not establish invalid-literal Parse parity. Explicit TEXT's existing
`operator does not exist:` error maps to 42883 in `app/error/execution.rs`.
The generated cast may add resource/cancellation precedence relative to the
old rejected literal; the generic array parser remains unqualified.
