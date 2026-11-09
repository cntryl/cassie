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

The same evaluated-carrier move applies to the JSON-to-JSON cast branch.
Target admission still precedes expression evaluation, and JSONB-involved
casts still run their existing value admission and canonicalizer in order.
The result of that canonicalizer is not qualified by the move, and its serde
private-sentinel behavior remains an open #809 semantic boundary. Test-first
pointer, NULL, scalar fallback, JSON/JSONB and canonicalization-error probes
remain unexecuted until the consolidated gate.

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

For scalar evaluation of a JSON column or parameter, when there is no general
leaf-memory owner but an expression/deletion owner and statement controls do
exist, the selected source handoff serializes the borrowed serde Value with
the checked compact writer and transfers its output reservation to that owner.
The source stays alive throughout serialization. If a general leaf owner is
present, or the expression owner/controls are absent, the existing path is
unchanged; this avoids adding a second charge over the general leaf estimate.
Jev selected this scope (0.98), with spelling/representation (0.79), source and
tracker overlap (0.72), and context/sibling error priority (0.62) as leads.
Receipt: `target/assembly-review/json-leaf-owner-contract-{request,response}.json`.
Test-first probes cover actual copy cancellation, exact/below overlap, foreign
trackers, general-owner/ownerless fallbacks and sibling failure. The shared
expression owner may conservatively retain intermediate output after its use,
and new resource/cancellation errors can precede old outcomes. Rich Value
clones, other consumers, source construction, allocator transient excess,
diagnostics, teardown and private-sentinel storage remain open. All probes
remain unexecuted pending the consolidated gate.

For a JSON-returning function used as a scalar, retain the existing direct
move of a raw `Value::String`; for other returned Values use the same scoped
controlled scalar serializer and then move its JSON carrier. The function is
still called once, NULL remains NULL, and general-owner/ownerless contexts
retain the established fallback. The original function-result Value remains
alive through serialization, but its construction and lease, and the owner
of direct raw String returns, are separate unqualified boundaries. Jev selected
this scope (0.99), with metadata/variant/fallback (0.82), overlapping owners and
failure cleanup (0.76), and call/order/NULL/cancellation (0.41) leads. Receipt:
`target/assembly-review/json-function-result-owner-contract-{request,response}.json`.
Authored builtin/user-function, exactly-once, variant/error, actual-copy and
budget/owner probes are unexecuted. Earlier resource/cancellation errors,
allocator transients, diagnostics, teardown and sentinel/storage migration
remain explicit.

Selected scalar JSON builders previously dropped their workspace reservation
as `Value::String` escaped. When a statement has an expression owner and no
general leaf owner, transfer that existing workspace lease to the expression
owner after `finish`, retaining the whole charge conservatively. Guard the
result before the lease on failures, verify tracker identity, and grow the
retained charge if the resulting String capacity exceeds it. This capacity
check happens after existing construction; it does not establish pre-admission
of all builder allocations or JSONB canonicalizer work. Other caller contexts
retain their existing path. The retained workspace can increase later budget
pressure, and the added pre-transfer interruption check may win over later
work. Jev selected this scope (0.99); receipt:
`target/assembly-review/json-builder-output-owner-contract-{request,response}.json`.
Test-first builder variant, post-finish cancel, synthetic over-capacity growth,
same-tracker/fallback and last-output-owner probes remain unexecuted. Original
argument cloning, canonicalization, allocator transients, errors and teardown
remain separate obligations; this does not close #809 or #811.

JSON mutation functions (`jsonb_set`, `jsonb_set_lax`, `jsonb_insert`,
`json[b]_strip_nulls`, and `jsonb_pretty`) likewise create a raw result while
holding a workspace lease. When the caller has an expression owner and no
general leaf owner, the selected path retains the existing lease with the
result through the same value-first guard, verifies tracker identity and grows
for an unexpectedly larger output capacity before transfer. The mutation's
branch-local scratch is dropped first. NULL, lax treatment, validation/error
order and ownerless/general-owner paths stay unchanged, except that an added
pre-transfer interruption may win over later work. The full workspace charge
remains conservatively retained. A post-allocation capacity grow cannot prove
pre-admission of prior allocations; source clones, JSONB canonicalization,
diagnostic allocation and teardown remain unqualified. Jev selected this
scope (0.98); receipt:
`target/assembly-review/json-mutation-output-owner-contract-{request,response}.json`.
Variant, error, cancellation, capacity and output-lifetime probes are authored
before implementation but unexecuted. This does not complete #809 or #810.

The finite `json[b]_extract_path[_text]` function forms likewise return a
String after dropping their preexisting extraction workspace. For an existing
expression owner with no general owner, retain that workspace using the same
value-first String guard after the source-document temporary is dropped. NULL
and error ordering, JSON versus JSONB formatting, and ownerless/general-owner
fallback remain unchanged apart from the added pre-transfer control check.
Jev selected this scope (0.98); receipt:
`target/assembly-review/jsonpath-result-owner-contract-{request,response}.json`.
Variant, missing/NULL/error, tracker, actual post-extraction cancellation,
over-capacity and last-owner probes are authored but unexecuted. Earlier
construction/pre-admission, output diagnostics, teardown and JSONPath core
execution remain unqualified. `jsonb_path_query_array/first` instead return
serde Value trees whose physical-capacity coverage is not established by a
serialized-size estimate; no tree ownership completion is claimed here.

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
