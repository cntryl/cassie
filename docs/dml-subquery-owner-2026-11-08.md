# DML row-subquery owner handoff

The selected partial repair keeps a row-subquery's query and operator owners
with its result values until each UPDATE or ON CONFLICT assignment conversion
has finished. The result directory is admitted before allocation and is
destroyed before its reservation. Zero-row results still produce one NULL
per assignment column, and the multiple-row cardinality error remains before
destination admission. Width validation still follows the move.

Destination-directory admission and per-value interruption can precede a
later width or conversion error. Source and destination leases overlap, and
the allocator may transiently return excess capacity before rejection.
The existing assignment JSON conversion and copies, general nested-value
accounting, and Midge internals remain outside this increment's coverage.

Jev's source-backed decision is recorded at
`target/assembly-review/dml-row-subquery-owner-contract-response.json`. The
test-first probes remain unexecuted pending consolidated validation.
