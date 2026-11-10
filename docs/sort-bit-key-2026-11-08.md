# Controlled BIT and VARBIT sort keys

For top-level native BIT/VARBIT with a matching declared or inferred bit
type, controlled SORT retains the copied key bytes instead of shrinking the
Typed key charge to zero. The existing conversion bound includes the source
BitString capacity twice and thus covers an exact-length output byte request.
The builder checks each byte chunk, preserves the original bit length and raw
padding bytes, and rejects a reported output capacity above its admission.
SEARCH lineage remains first and other shapes retain their existing path.

This can add interruption/resource errors or retain more query budget than
before. Allocator excess may be observed only after allocation. Earlier input
cloning, nested BIT keys, type inference, comparisons and tie rendering are
not qualified by this increment. Jev's decision is recorded at
`target/assembly-review/sort-bit-key-contract-response.json`. Focused parity, empty/padding, owner/budget,
actual-copy cancellation and SEARCH probes are unexecuted pending the
consolidated validation gate.
