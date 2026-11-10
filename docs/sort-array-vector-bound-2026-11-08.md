# SORT ARRAY(Vector) key admission

For a native SQL Array declared as Array(Vector), containing only Vector or
NULL elements, the controlled SORT path separately admits the dimensions,
outer semantic slots and one SemanticValue slot per vector component before
constructing the key. This covers the conversion expansion that the prior
input-size estimate missed. Each output directory is allocated for its exact
element count and rejected if reported capacity exceeds that count. Components
use the same f32-to-f64 semantic normalization as the existing key builder.

SEARCH lineage is checked once and retains precedence. Other shapes use the
existing key path. Additional pre-admission can reject tighter budgets, and
component checks can report cancellation earlier. Allocator excess can occur
transiently before rejection. Type inference, general key construction,
comparison, tie sizing, other nested/Typed keys and Numeric BigInt capacity
and normalization are not qualified by this increment.

The focused decision is recorded at
`target/assembly-review/sort-array-vector-bound-contract-response.json`.
Test-first wide-vector, dimensions, special-float, fallback, lineage, budget
and actual-component cancellation probes remain unexecuted pending the
consolidated validation gate.
