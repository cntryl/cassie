# Controlled JSON foundation: proposal and remaining proof

This records selected internal parser design directions and their outstanding
proof obligations; the full parser is not implemented or qualified. The
checked serde reader and a value owner form a useful partial increment, but
do not make every parse, comparison, conversion, error, or output operation
interruptible or fully admitted. See the Jev receipts
`target/assembly-review/json-parser-foundation-{request,response}.json` and
`target/assembly-review/json-parser-complete-{request,response}.json` in the
coordinator worktree. No probes described here have been executed.

## Observable boundaries

Binary JSON input currently validates the entire wire byte slice as UTF-8 in
`pgwire/connection/codecs.rs::binary_to_value`, parses it with
`serde_json::from_str`, and returns `Value::Json`. The existing codec maps both
UTF-8 and JSON errors to its binary invalid-data diagnostic. Its output path
renders the resulting serde value, so duplicate fields and original token
spelling are not retained. A replacement must preserve these behaviors and the
priority of invalid UTF-8 over JSON parsing errors.
Pinned serde's Value deserializer also classifies special private Number and
RawValue object keys in its first-key path; a generic duplicate-last object
parser cannot assume those cases are ordinary fields. Its arbitrary-precision
number scanner normalizes an exponent's `E` to `e` and inserts an explicit
positive sign in certain spellings. Source and differential probes must capture
those behaviors rather than merely preserving the raw input token.
In particular, a first `$serde_json::private::Number` key routes through
`NumberFromString`, while a first `$serde_json::private::RawValue` key routes
through `BoxedFromString` and calls a fresh `serde_json::from_str` on its
contents. The latter can reset serde's recursion counter for the nested source.
Compatibility frames must account for decoded-source ownership, nested error
locations and this depth behavior; rejecting these spellings or treating them
as ordinary maps would change the existing Value contract. A sentinel in a
later object position also needs a separate differential witness.

JSONB canonicalization is a different consumer: `types/jsonb.rs::canonical`
parses the complete document before formatting, then orders surviving object
fields by key byte length and bytes, formats numbers via NUMERIC, and rejects a
decoded NUL with SQLSTATE 22P05. Invalid JSON uses its 22P02 diagnostic. JSON
deletion has its own path and numeric-error precedence; it cannot be treated as
an interchangeable canonicalizer. Existing serde recursion behavior, including
its depth boundary and error position, must be preserved for each consumer.

Pinned serde_json 1.0.154 reads a checked reader byte by byte, but validates a
complete decoded string with `str::from_utf8` after input consumption. Its
object maps perform infallible String comparisons, and recursive values may be
dropped recursively. The binary codec also performs its own full-slice UTF-8
check before parsing. Thus a checked reader bounds input-read progress, not all
subsequent parser CPU or destruction. The existing 192-times-serialized-byte
reservation is documented in `runtime/accounted/json.rs`; allocator excess,
map split and duplicate replacement peaks, overlapping scratch and retained
values have not been proved for this application.

## Candidate for full control

One candidate is a Cassie-owned iterative tokenizer and value representation
whose frames, strings, numbers and ordered-object directories each retain a
capacity-coupled query reservation. An incremental UTF-8 and escape decoder
would preserve partial code points across chunks. Object lookup and duplicate
replacement would use a fallible, chunk-checked key comparator. Each growth
must admit old/new overlap, check potentially long moves, and keep the old and
new allocations attached to their owners on cancellation and errors. The
representation must support iterative destruction so a dropped document cannot
perform an unchecked recursive walk. This is a candidate to assess, not a proof
that the current consumers can accept it.

The consumer boundary is essential. Converting the result to
`serde_json::Value` still invokes infallible BTree comparisons and exposes
recursive destruction; it therefore cannot establish full interruption control
merely by following an iterative tokenizer. A complete design must carry the
owned representation through each relevant consumer and output serializer, or
provide and explicitly qualify a separately controlled handoff. It must not
replace JSON semantics with JSONB formatting or assume deletion's walker is a
general canonical parser.

Before implementation, independently establish how every malformed token,
line/column, duplicate field, number spelling and depth failure maps to the
existing observable result, and how parsed values, scratch, diagnostics and
network/output owners overlap. Record any public representation or compatibility
change before using it. Author differential tests for escaped UTF-8 and
surrogates, duplicate keys, nested arrays/objects, arbitrary precision,
recursion boundaries, invalid UTF-8 and malformed-before-format precedence;
add exact/one-below budget, duplicate replacement, mid-token, mid-comparison,
mid-copy and teardown cancellation probes synchronized at actual work. No
smaller literal-size limit is part of this proposal.

## Arena contract to resolve before implementation

The candidate representation is a private, in-memory arena, not a persisted
format or another storage layer. Each frame identifies its source buffer,
cursor, serde-compatible recursion state, syntactic context and destination.
Each source, token, node, key, child directory and output buffer must have an
owner that outlives every borrowed view. A replacement allocation must be
admitted while its predecessor remains charged; cancellation during a move
must drop each live allocation before its lease. Releases, including partially
constructed documents and nested decoded sources, need an iterative ownership
plan that does not depend on walking an arbitrarily deep Rust value tree.

Jev selected an internal owner-coupled arena and structured owned diagnostics
(0.97); see `target/assembly-review/json-arena-api-{request,response}.json` in
the coordinator worktree. The intended internal entry points are a UTF-8
source parser and a binary-input wrapper that checks all bytes before syntax.
The result owns its nodes and nested decoded sources, retaining any borrowed
outer input's existing owner for the full arena lifetime. Each allocation
precedes its reservation in its structural owner. The parser distinguishes
interruption and resource errors from invalid input. Invalid input carries an
owned diagnostic with serde-compatible category, line, column and any
custom/nested message, retaining the message reservation through every caller
handoff. Consumers map that diagnostic to their existing SQLSTATE and message;
binary COPY's invalid-data mapping and JSONB's TypeInput mapping remain
distinct. The exact Rust interface may remain crate-private, but it must make
those ownership and error branches explicit.

This design does not rely on fabricating `serde_json::Error`: its syntax/EOF
codes and position-fixing constructors are private in pinned serde_json
1.0.154. `serde::de::Error::custom` can carry a message and position but
classifies it as Data, so it cannot reproduce arbitrary syntax or EOF category.
The custom diagnostic's text, categories, nested-error wrapping, caller
conversion and error-memory handoffs require source and differential proof;
the selection is not evidence of that parity. Existing general resource-error
ownership is likewise not claimed complete.

The tokenizer must reproduce the existing consumer's operation order. Binary
COPY must check all input bytes as UTF-8 before attempting JSON syntax, because
that is the existing codec order; incremental UTF-8 validation must carry an
unfinished code point between chunks. Within JSON syntax, count lines and
columns, validate escapes and surrogate pairs, enforce serde's existing ordinary
nesting limit and track its error position. A RawValue sentinel creates a new
decoded source and a fresh serde parse context: retain its input and lease until
that nested parse returns, propagate its error through the same wrapping route,
and resume or reject the enclosing object as serde does. The nested parse runs
before the enclosing `end_map`, so a malformed decoded RawValue must win over a
following outer member or trailing-comma error. Do not introduce a
global substitute depth cap that rejects currently accepted sentinel nesting.

For ordinary objects, classify only the first decoded key against the two
enabled serde private tokens. `NumberFromString` requires a JSON string whose
contents are parsed as a Number; `BoxedFromString` requires a JSON string whose
contents are parsed afresh as a Value. Those visitor branches return before
reading another member, and serde's `end_map` then rejects a following comma.
For a non-sentinel first key, later token-shaped keys are ordinary fields.
Ordinary duplicate fields keep the last value. Retained numbers must follow
serde's `scan_integer` and `scan_exponent`, including its small-integer path,
`-0`, conversion of `E` to `e`, insertion of `+` where absent, and arbitrary
precision; a borrowed raw token is not always the retained representation.

Jev selected an incremental checked balanced tree as the object-storage design
direction (0.73); see `target/assembly-review/json-arena-object-{request,response}.json`
in the coordinator worktree. Retain-and-sort would keep duplicate values longer
and can alter peak budget and the order of resource versus syntax failures;
a digest index adds an unproved hashing and collision-security contract. The
selection is not a proof of an implementation.

The candidate tree uses stable arena indices rather than pointers into movable
buffers, fallible bytewise lexical key comparison in bounded chunks, and
checked, admitted node/path-directory growth. Keep the tree balanced and prove
its ordering, height and rotation invariants; do not add a field-count or depth
cap. Parse and own the entire new member value before key comparison or map
mutation: serde's `next_entry` completes that parse before `Map::insert` and
can therefore report its syntax error first. A duplicate keeps the already
stored key, discards the candidate key and replaces the previous value only
after the new one exists; both values and their leases coexist during that
handoff. An interrupted insertion must either leave a valid reusable tree or
abort and safely destroy the entire unpublished object without unleased data.
Plan and admit iterative disposal of displaced values and partial nodes; do not
assume recursive destruction or an in-place Vec reallocation is interruptible.
Iteration must match serde's lexical String order for compact JSON; JSONB's
later length/byte sort remains a distinct consumer operation. Exact budget,
allocator-excess, sentinel, error and teardown behavior remains to be proven.

The arena must not be materialized into an ordinary `serde_json::Value` and
then described as fully controlled: serde's Value map insertion and recursive
drop are independent unchecked boundaries. Each caller needs an explicit
handoff: compact binary JSON output must use serde Value's number, escaping and
map order; JSONB must finish parsing first, use length/byte key order, reject
surviving decoded NULs and retain NUMERIC diagnostics; deletion must preserve
its selected-path/error precedence. For every handoff, the resulting value or
text and its reservation must remain together through error, clone, return and
transport ownership boundaries. Record external/public compatibility changes
before selecting them.

The differential probe matrix must include both token names in first and later
positions, valid and malformed sentinel payloads, a sentinel followed by a
second member, and recursively encoded sentinel strings that cross the ordinary
serde depth boundary. Check exact error class, message and line/column against
the current consumer, not merely success. Also cover invalid UTF-8 preceding a
syntax error, partial multibyte boundaries, lone and paired surrogates,
malformed/trailing commas, duplicate keys and escaped-equivalent keys,
arbitrary-precision and exponent spellings, JSONB NUL/numeric precedence, and
ordinary deep arrays and objects. Resource probes must synchronize after real
UTF-8/escape scan, source or node allocation, duplicate replacement, key
comparison, output copy and partial-frame teardown; verify exact/below limits
and reservation lifetime through successful and failed consumer handoffs. All
such probes remain prospective until authored and executed at the consolidated
gate.

Minimum sentinel witnesses, with expected behavior to be checked against the
pinned serde oracle rather than inferred from this table alone:

| Input | Source-backed branch to compare |
| --- | --- |
| `{"$serde_json::private::Number":"12E2"}` | First key invokes NumberFromString and its number scanner. |
| `{"$serde_json::private::Number":12}` | First-key value must satisfy the NumberFromString string visitor. |
| `{"$serde_json::private::Number":"12","x":0}` | Visitor returns after the sentinel value; enclosing `end_map` encounters a comma. |
| `{"x":0,"$serde_json::private::Number":"12"}` | The sentinel appears after an ordinary first key and is an ordinary map key. |
| `{"$serde_json::private::RawValue":"[1,2]"}` | First key parses the decoded string as a fresh JSON source. |
| `{"$serde_json::private::RawValue":"[1,"}` | Nested parse error is wrapped at the original consumer boundary. |
| Nested RawValue sentinel strings around ordinary arrays | Each fresh parse has its own recursion state and decoded-source owner. |

Also vary escaped spellings of the sentinel key and value; classification is
on the decoded key rather than raw token bytes. Distinguish an explicitly empty
sentinel string, non-string sentinel value, trailing input inside the decoded
source, and trailing members outside it. Exact diagnostics and source positions
are still to be established by the deferred oracle probes.

## Compact Value output increment

Jev selected an independent compact `serde_json::Value` writer for source
implementation; see `target/assembly-review/json-compact-writer-{request,response}.json`
in the coordinator worktree. It borrows an already-owned Value and emits exactly
the pinned serde compact JSON representation: map iteration order, escaping,
and number spelling under the enabled `arbitrary_precision` feature. Its
iterative work frames and returned String have separate reservations. Growth
admits old/new overlap, keeps each allocation paired with its lease on failure,
and checks bounded scans and moves. The source Value and its separate owner
remain alive throughout serialization; the returned String and its reservation
travel together. There is no new document depth or literal-size cap.

This increment does not qualify input parsing, construction or destruction of
the borrowed Value, JSONB canonical formatting, or transport and network
buffers. Its consumer handoff must be wired and reviewed separately before a
COPY path is claimed controlled. Focused parity, capacity, lifetime and
synchronized cancellation probes are authored first and remain unexecuted
until the consolidated validation gate. `try_reserve_exact` may transiently
return allocator capacity larger than requested; the candidate rejects that
allocation and drops it, but does not prove pre-admission of that allocator
excess. The `into_parts` compatibility handoff is valid only if the caller
keeps the String and lease coupled through every later success and error path.
