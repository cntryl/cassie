# JSON deletion through an owned arena: decision record

This record investigates the end-to-end `-` and `#-` routes in #809. It is
not an implementation or evidence that the issue is complete. The source
baseline is `92d163511827a46a1d278a12452ddb12b5d3ae57`; all new Cassie
regressions remain unexecuted until the consolidated validation gate.

## Observed contracts

Cassie's feature-support table specifies deletion for both JSON and JSONB,
retains the source OID, and documents strict SQL NULL, path, overload, numeric,
and error behavior. The JSON form is a Cassie extension: a PostgreSQL 18.6
standalone oracle reports `42883` for `'{"a":1}'::json - 'a'`. The same oracle
rejects `'{"a":"\u0000"}'::jsonb` with `22P05` and a JSONB input containing
`1e131072` with `22003` during the input cast. For duplicate object keys,
`'{"a":1,"a":2,"drop":0}'::jsonb - 'drop'` returns `{"a": 2}`.

The existing filter route evaluates left then right, handles SQL NULL and
array dimensionality, and uses `json_delete` and `json_document` for the
actual operation. `RawObject` uses a custom map visitor, with last duplicate
key winning; its first key is always a normal key. Format recursively sorts
keys by UTF-8 byte length then bytes, serializes decoded NUL as `\u0000`, and
maps numeric input failures to `QueryError::General`. Path elements are
checked lazily: missing paths and scalar descendants avoid consulting later
elements. JSONB casts ordinarily invoke `types::jsonb::canonical`, which has
different `22P05`, `22P02`, and numeric error behavior before the operator.
These distinct sources of errors must not be conflated.

The integrated arena is an owned `serde_json::Value`-mode parser plus a JSONB
formatter. Its first-key `$serde_json::private::Number` and
`$serde_json::private::RawValue` sentinels are not ordinary `RawObject` keys;
its JSONB formatter rejects NUL and reports numeric overflow through its own
error enum. The forest has admitted lookup, but no public deletion operation.
An unmodified arena parse/format pipeline is therefore not equivalent to the
current JSON deletion route. Parsing or validating every number before path
selection may also change which error wins.

The pinned serde_json 1.0.154 `ignore_value` used by `RawValue` and the JSON
cast's `IgnoredAny` validates with an iterative scratch stack and does not
apply the Value deserializer's remaining-depth counter. `SourceStack` in the
arena Value mode does apply that counter. Reusing it unchanged would reject
documents the old raw scan can accept; deletion parsing needs its own no-new-
limit policy as well as matching syntax/error ordering.

The raw scan's `ignore_escape` consumes any syntactically valid `\uXXXX`
without checking surrogate pairing (serde_json 1.0.154 `read.rs:1076–1095`).
The Value arena eagerly decodes strings. A removed `"\ud800"` subtree can
therefore succeed on the old deletion route but fail under eager arena
decoding. The deletion representation needs raw or lazily decoded values;
sentinel and depth flags alone are insufficient. A draft flag-only parser is
preserved for investigation and is not wired or accepted.

## Selected design

A focused Jev choice selected a separate RawObject-compatible arena parse and
deletion formatter with a non-mutating selection view (0.99). Its parity (0.95),
ownership (0.57), control (0.81), and diagnostic/OID (0.89) leads require the
focused probes below. The request and response are recorded under the ignored
`target/assembly-review/json-deletion-arena-contract-{request,response}.json`.
This selection is a design direction, not executable proof. The design must
preserve the documented JSON extension, source OID, ordinary sentinel keys,
duplicate-last behavior, lazy path checks, child evaluation order, and the
result owner's lifetime through every controlled query consumer. It must not
add a depth limit. The full raw document syntax is validated before output;
only surviving values undergo deletion formatting. JSONB cast errors remain
at the input cast boundary, before the operator.

A subsequent source probe exposed the eager-surrogate counterexample above.
The refined Jev decision selected deletion-specific checked raw spans and
on-demand traversal (0.90; lazy forest 0.07), with parity 0.63, ownership 0.63
and remaining-control 0.70 requiring targeted probes. Receipt:
`target/assembly-review/json-deletion-lazy-raw-contract-{request,response}.json`.
The syntax-only scan must not impose the Value-mode depth cap or decode
surrogates. Each reached object's keys must all be decoded before its
lookup/removal, with the last duplicate retained. A removed or unreached
subtree's keys and values remain opaque. Array string-key matching treats an
invalid decoded candidate as a nonmatch; a surviving string is subsequently
decoded and may report an error. Output visits surviving members in the old
canonical key order, preserving numeric/error precedence. The preserved
eager arena flag draft is not selected for integration.

The current worktree contains a test-only raw syntax scanner draft with an
admitted iterative frame stack and borrowed result spans. It is not connected
to SQL deletion. Exact malformed diagnostics, synchronized stack-move
cancellation, raw object indexing, selection, formatting and result-owner
handoffs remain open; no executable tests have been run. A pre-cancelled query
reports cancellation before inspecting even an empty or malformed input.

Required probes include JSON versus JSONB casts and operators; NUL in removed
and surviving fields; duplicate and sentinel keys; malformed documents and
overflow in removed versus surviving values; empty/missing paths, NULL and
bad later indexes; exact numbers; source/target OIDs; cancellation after real
scan/output work; exact/below admission; and result ownership across nested
expressions and each consumer. Recursive teardown, allocator excess and
unsupported consumer handoffs remain explicit until separately proved.
