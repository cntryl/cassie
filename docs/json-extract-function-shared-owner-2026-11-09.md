# JSON extraction functions with one expression owner

Status: scoped implementation in progress; executable checks are deferred.

The aggregate window argument route can carry one `EvaluationMemory` as both
general and JSON/deletion owner. The builtin dispatcher currently suppresses
the result owner when a general owner exists, so the four JSON/JSONB
`extract_path` and `extract_path_text` functions use their legacy fallback
instead of the existing checked raw-document selection and output owner.

The selected increment passes the shared owner only to those four registered
`ExtractPath` variants when both owner references are pointer-identical. It
retains existing deletion-only behavior, distinct-owner/ownerless fallbacks,
other JSON families and declared-function precedence. The existing raw scan,
result-owner tracker check and admitted transfer remain responsible for their
own contracts. General expression admission and extraction workspace/output
reservations can overlap, so reached expressions may observe earlier budget or
cancellation failures than the old fallback. NULL/error ordering, output
spelling and identity must remain unchanged for successful controlled calls.

Jev selected this four-variant, same-owner scope (1.00); its 0.54 gap requires
overlap, tracker, actual scan cancellation and error-priority probes. Receipt:
ignored `target/assembly-review/json-extract-function-and-array-parse-*`.
Existing input `serialized_json_bytes`, JSONB length and `json_text` traversals,
source `Value` construction, generic result-to-scalar conversion, allocator
transients and teardown remain unqualified. This does not complete #806 or a
general JSON resource-control contract. All authored Cassie probes are
unexecuted until the consolidated gate.
