# Rollup Lifecycle

Cassie rollups are local, Midge-backed derived tables. The source table remains authoritative; query planning may substitute rollup rows only while persisted metadata proves the rollup is ready for the current source generation.

## Durable definition format

`RollupMeta.version = 1` identifies the 0.1.0 definition format. Each `aggregates[].expression` stores canonical, parseable SQL for the aggregate expression, such as `max(("amount" * 10))`. It is source text, not a matching key. Canonical output uses lowercase function names, double-quoted identifiers with doubled embedded quotes, single-quoted strings with doubled apostrophes, spaces around operators, and explicit parentheses for composite expressions. The stored `function`, `alias`, and `data_type` fields continue to describe the aggregate output.

Rollup matching reparses each stored expression and derives the same transient aggregate signature used for the query plan. The signature is never persisted as the expression. Refresh and restart recovery also parse the stored SQL, so an aggregate such as `MAX(amount * 10)` is rebuilt from the original expression tree rather than from a lossy name assembled from its operators.

An earlier unreleased version-2 definition stored aggregate signatures in `expression`; those strings cannot always recover the original expression tree. Cassie does not infer or migrate that pre-release state. A pre-0.1.0 store must be recreated under the new layout epoch. An unparsable version-1 expression fails closed: it cannot be substituted, and `REFRESH ROLLUP` returns an error while source reads remain available.

`output_key_format = 1` identifies rows keyed by the serialized bucket and group-key tuple. Older metadata defaults to format zero, which remains readable; the first successful full refresh replaces ordinal row IDs and publishes format one with the refreshed rows.

## Build, publication, and recovery

1. A create or refresh reparses the definition, computes all grouped rows, and converts every output value to JSON before changing persisted rollup state or output rows.
2. Non-finite FLOAT values are rejected with an error during this preflight. They are never encoded as JSON null. A failed `CREATE ROLLUP` removes the new metadata and output collection. A failed refresh leaves the existing metadata and output rows untouched; if that generation is still current, it remains usable.
3. After preflight succeeds, Cassie takes the source and output Midge write gates, verifies that the definition and source generation are still current, and persists `Building`. It recreates the output collection, then commits the complete output row set and `Ready` metadata together in one Midge Data transaction. Concurrent source writers and publishers for that rollup serialize at these gates; different rollups can publish independently. A `Building` or stale rollup is not eligible for substitution, so reads use the authoritative source.
4. After a successful source mutation, Cassie derives old and new daily group identities from a bounded source-write report. Incremental maintenance is eligible only for a ready rollup exactly one source generation behind, with a one-day UTC bucket, one non-null TEXT group key, a matching fixed-UTC time-series index, and `COUNT(*)` plus bare integer-column `SUM`, `MIN`, or `MAX`. It recomputes only the affected buckets, checks each prior count against the committed row-count delta and selected index membership, then reads authoritative rows for those buckets. Zero-net corrections still recompute their group; an emptied group removes its derived row. Reports over 256 groups or 64 KiB, missing hints, generation gaps, unsupported definitions, or any selected-bucket integrity mismatch use a full refresh and record the fallback reason.
5. If source-write maintenance fails after the source commit, the durable rollup-maintenance debt keeps the source authoritative and is retried from the source on startup. Recovery reparses the canonical aggregate SQL and publishes `Ready` only after a complete rebuild. A failed explicit refresh after replacement begins remains non-ready; the operator can retry `REFRESH ROLLUP` after resolving the storage or resource failure.

See [Production Readiness](production-readiness.md#rollup-and-retention-operator-contract) for operator diagnostics and the retained capacity boundary.
