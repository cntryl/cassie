# WIDTH_BUCKET mixed NUMERIC and FLOAT8 gap

PostgreSQL 18.6 selects FLOAT8 when NUMERIC and FLOAT8/REAL operands are
mixed. At 2^53, `width_bucket(9007199254740993::numeric,
9007199254740992::float8,9007199254740994::numeric,2)` returns 1; Cassie's
current runtime selects NUMERIC whenever any operand is NUMERIC and therefore
follows the exact-arithmetic path. PostgreSQL coerces NUMERIC to FLOAT8 before
strict NULL and count processing: finite overflow or underflow reports 22003
even beside NULL or a zero count, while NUMERIC infinities survive conversion.
These oracle probes were executed against PostgreSQL and its server was stopped.

Jev selected holding the mixed-family runtime change until its representation
and diagnostic ownership are source-proven (receipt
`target/assembly-review/width-bucket-mixed-family-contract-*`). The remaining
Cassie-side work is to identify a private, non-spoofable selected-overload
marker that survives binding and argument evaluation; preserve coercion order,
no-controls behavior and PostgreSQL's exact numeric text in range diagnostics;
and retain every conversion and error/frame lease through pgwire writes.
The public serde AST cast identity is exhaustively matchable, and the existing
branchless-CASE internal wrapper can be forged by manually constructed public
ASTs. The controlled numeric formatter has documented allocator, arithmetic
cost and core FLOAT8 parse-interruption limits; the existing self-leased boxed
numeric diagnostic has a small owner-deallocation accounting boundary. None of
these is an unavailable Midge API. This remains open Cassie-side #813 work; the
integer-only correction does not close it.
