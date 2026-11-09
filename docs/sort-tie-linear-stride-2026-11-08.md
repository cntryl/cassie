# Linear SQL-array tie stride traversal

The controlled tie serializer computes the root suffix stride once and
carries the stride in each array traversal frame. For a child of positive
length, its stride is its parent's stride divided by that child length; a
zero-length child never descends and uses zero. A reachable zero makes all
ancestor strides zero, so later negative dimensions remain unvisited. The
existing preflight ensures no-zero products fit the element count; malformed
or insufficient shapes preserve the established nested-JSON fallback.

The root suffix scan is linear in dimension depth and per-frame stride work
is constant. Actual output traversal still scales with emitted elements and
arrays. Each frame grows by one stride field, which can change scratch budget
rejections. Cancellation is checked per frame, but its exact timing changes
from the previous repeated scans. Numeric formatter CPU, typed formatting,
allocator excess and teardown remain separate unqualified boundaries.
Jev's choice is recorded at
`target/assembly-review/sort-tie-carry-stride-contract-response.json`.
Parity/deep/zero/malformed and counted-stride-work probes are authored first
and remain unexecuted until the consolidated validation gate.
