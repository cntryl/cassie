# Controlled JSON extraction

The owned expression-input follow-up moves an already-evaluated JSON String
into the extraction carrier rather than cloning it. The same move applies to
both typed concatenation operands; left/right conversion order and non-JSON
scalar conversion/error behavior remain unchanged. The surrounding evaluation
owner continues to cover an admitted input until the branch completes. Other
source-column clones, scalar-to-JSON serialization, casts and consumers remain
separate ownership obligations. Authored pointer, output/error and owner probes
are unexecuted until the consolidated gate; moving a non-JSON operand into the
helper can retire its temporary source earlier after conversion.

Controlled extraction and typed concatenation also need an owned conversion
for non-JSON scalars. The selected candidate keeps each converted value before
its own reservation, retaining the left operand while converting the right
and retaining both through the operator. JSON and NULL still move; no-controls
callers retain the existing conversion. Strings use the checked compact JSON
quoting sink. Finite floats and integers serialize into a fixed 64-byte stack
buffer before a checked sink copy; nonfinite floats keep the existing error.
Pinned serde_json 1.0.154 uses the same `zmij::Buffer::format_finite` for both
`Number::from_f64` and its f64 serializer; pinned zmij's buffer is 24 bytes,
and i64 decimal text fits 20 bytes. This is a source proof for that pinned
implementation, not executable qualification. Test-first probes cover float
spelling/extremes, escapes, NULL/NaN/incompatible errors, actual copy
cancellation, exact/below admission and left/right/result owner failures.
Jev selected the scoped design (0.97); receipt:
`target/assembly-review/json-operand-conversion-contract-{request,response}.json`.
Extra admission/cancellation may precede legacy errors; source column clones,
unowned callers, allocator transient excess, diagnostic allocation and
teardown remain open. All new probes are unexecuted.

This is a scoped implementation contract for #809, not an issue-completion
claim. The source baseline is the tree of `dd74bbac`; executable qualification
is deferred until the consolidated milestone bundle is complete.

`->`, `->>`, `#>` and `#>>` currently parse the entire raw document before
traversal, decode every key of each reached object and select the last matching
duplicate. An array path index accepts leading ASCII whitespace and an INT4
decimal with optional sign; an invalid or out-of-range index produces SQL NULL,
as does a missing key or NULL path element. For JSONB only, direct integer
selectors 0 and -1 return the scalar itself. Native path element type checks
precede document parsing; the string-backed path is parsed after the document.
The existing expression dispatcher calls `as_json_document` before extraction.

JSON result operators preserve the selected raw spelling, while JSONB results
format the selected value using the existing raw formatter. Text result
operators return SQL NULL for JSON null, decode selected strings and otherwise
use the corresponding JSON raw or JSONB formatted representation. JSON and
JSONB cast validation and source OID remain distinct. Malformed documents,
reached-key decoding errors, selector conversion and NULL precedence must match
the current entry point. No Value-mode sentinel interpretation is introduced
for the raw document.

The selected Jev direction is borrowed checked raw traversal followed by an
independently admitted raw, decoded or formatted output; receipt
`target/assembly-review/json-extraction-raw-contract-{request,response}.json`.
The original document and path owners must outlive all borrowed spans and the
output lease must be transferred to each supported expression consumer. A
checked syntax scan validates the full document before selecting, reached
objects retain decoded key owners through lookup, and arrays are traversed
without building an unowned directory. Existing selectors and consumers not
proved by a particular increment remain on the legacy path. New admission and
interruption checks can take precedence over later semantic errors; those
changes must be identified at each production handoff.

The first production handoff covers direct text/integer selectors and
native path arrays containing only TEXT/NULL. Incompatible native element
types and callers without a result owner continue through the legacy route.
Supported native paths are validated before the
document scan; checked validation can surface cancellation before a later
type or syntax error. Direct selectors scan after `as_json_document` as before.
Projection detection allocates a result owner for extraction expressions;
that owner remains attached through the projected row and its clones.
Existing document cloning, nested expression transformations and other
consumer handoffs are not qualified by this increment.

A subsequent scoped handoff reuses the admitted flat-brace and JSON-array
TEXT[] path owners after raw-document validation. Flat parser syntax errors
select the legacy route so its extraction-specific `ArrayInput` diagnostics
remain authoritative; memory and interruption failures propagate. The JSON
arena candidate already distinguishes semantic fallback from control failures.
Each path owner remains live through selection and output construction. Added
path admission and a post-parse interruption check can precede later traversal
errors. Nested/dimensioned brace forms, diagnostic ownership and fallback
parsing remain pending. Jev selected the scoped handoff (0.50); its precedence,
grammar, owner and resource leads require the authored probes and source review.
Receipt: `target/assembly-review/json-extraction-text-path-contract-{request,response}.json`.

Focused probes cover duplicate and escaped-equivalent keys, raw spelling and
JSONB formatting, JSON null, invalid and negative indexes, scalar 0/-1,
native and string-backed paths, invalid operand precedence, sentinel keys,
malformed or lone-surrogate survivors, exact/below budget and synchronized
actual scan, copy and owner-transfer cancellation. Preserve source/output
overlap through every error path. Diagnostic allocation, allocator transient
excess, full parser teardown and unproved expression transformations remain
explicitly unqualified. Jev and source review do not replace deferred tests.
