# JSON scalar leaves with one expression owner

Status: implementation in progress; Cassie probes remain unexecuted.

The EXISTS-window aggregate control work passes one `EvaluationMemory` as both
general expression owner and JSON/deletion owner. `json_scalar_leaf::from_value`
currently uses the checked compact serializer only when the general owner is
absent. With both owners present, `Value::Json` is serialized by an unchecked
`to_string()` before the controlled JSON extraction scan begins. The same-owner
source argument evaluators also reach this fallback.

The selected increment extends the already reviewed compact serializer path
only when the two owner references are pointer-identical, controls are present,
and the shared owner belongs to those controls. The serialized text and its
reservation must remain paired until the reservation is adopted by that owner;
the owner must outlive use of the returned scalar. The existing general leaf
admission remains, so its reservation overlaps the serializer's separately
admitted output. Reached work can therefore fail earlier for budget or
cancellation than the old fallback. Deletion-only behavior stays intact;
distinct owners and ownerless/no-control contexts retain their previous paths.

Jev selected this exact same-owner slice (0.98) over the broader alternative.
Its 0.74 lead requires same/foreign tracker, differing owners, JSON escaping and
sentinel-shaped Values, actual serialization cancellation, exact/below-budget
overlap, result/owner release, error cleanup, and an EXISTS-window false-FILTER
witness. Receipt: ignored `target/assembly-review/806-json-same-owner-*`.
This change does not qualify construction of the input JSON tree, JSON function
result-owner routing, allocator transient excess, synchronous teardown, or the
complete #806 acceptance criteria.
