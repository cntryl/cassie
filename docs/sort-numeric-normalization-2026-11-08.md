# Controlled NUMERIC sort-key normalization

For a finite native NUMERIC with a declared/inferred NUMERIC type, the
controlled SORT path admits an output-digit request before key construction
and separately admits an overlapping u32-limb workspace. It borrows the
input's digits, checks interruption during each digit copy, divisibility scan
and in-place division, and preserves sign, zero normalization, trailing-zero
removal and the existing NumericKey representation. Normalized values that
fit i64 use the same integer semantic key as the old path, including zero;
the bounded conversion avoids allocating a bigint or power of ten for that
check. SEARCH lineage remains first and other types and specials keep their
old route.

The requested output bound uses the pinned num-bigint 0.5.1 implementation:
32-bit digits request at most four bytes per u32 limb; 64-bit digits pack two
u32 limbs and request at most eight bytes per pair. Both are covered by
rounding the u32 count up to an even number and multiplying by four. The
input, workspace and output request overlap in the query budget. Scratch and
new cancellation points can reject earlier; extreme trailing-zero inputs may
still have quadratic CPU cost.

This is partial: `BigInt::from_slice` performs a whole-slice copy/conversion
without an interruption point, and the bigint's private actual capacity and
allocator over-allocation cannot be observed or proven pre-admitted here.
Generic arrays/nested numerics, type inference, comparisons, tie rendering and
the source Value's earlier evaluation remain separate gaps. The existing
uncontrolled route remains for other shapes; no supported type is rejected
because a broad bound is unknown.

The conditional Jev decision is in
`target/assembly-review/sort-numeric-key-contract-response.json`. Test-first
differential/order/hash, source/lease overlap, SEARCH, actual-division
cancellation and exact/below probes remain unexecuted pending the consolidated
validation gate.
