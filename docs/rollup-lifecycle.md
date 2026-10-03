# Rollup Lifecycle

Cassie rollups are local, Midge-backed derived tables. The source table remains authoritative; query planning may substitute rollup rows only while persisted metadata proves the rollup is ready for the current source generation.

## Durable definition format

`RollupMeta.version = 2` identifies the current definition format. Each `aggregates[].expression` stores canonical, parseable SQL for the aggregate expression, such as `max(("amount" * 10))`. It is source text, not a matching key. Canonical output uses lowercase function names, double-quoted identifiers with doubled embedded quotes, single-quoted strings with doubled apostrophes, spaces around operators, and explicit parentheses for composite expressions. The stored `function`, `alias`, and `data_type` fields continue to describe the aggregate output.

Rollup matching reparses each stored expression and derives the same transient aggregate signature used for the query plan. The signature is never persisted as the expression. Refresh and restart recovery also parse the stored SQL, so an aggregate such as `MAX(amount * 10)` is rebuilt from the original expression tree rather than from a lossy name assembled from its operators.

Version 1 metadata stores aggregate signatures in `expression`; those strings cannot always recover the original expression tree. Cassie does not infer or migrate those definitions. Version 1 rollups are excluded from query substitution and cannot be refreshed. Drop and recreate them to use the version 2 format. An unparsable version 2 expression also fails closed: it cannot be substituted, and `REFRESH ROLLUP` returns an error while source reads remain available.

## Build, publication, and recovery

1. A create or refresh reparses the definition, computes all grouped rows, and converts every output value to JSON before changing persisted rollup state or output rows.
2. Non-finite FLOAT values are rejected with an error during this preflight. They are never encoded as JSON null. A failed `CREATE ROLLUP` removes the new metadata and output collection. A failed refresh leaves the existing metadata and output rows untouched; if that generation is still current, it remains usable.
3. After preflight succeeds, Cassie takes the output collection's Midge write gate, verifies that the definition and source generation are still current, persists `Building`, replaces the output rows, and publishes `Ready` metadata with the source generation only after the replacement succeeds. Concurrent publishers for one rollup serialize at this gate, while different rollups can publish independently. A `Building` or stale rollup is not eligible for substitution, so reads use the authoritative source.
4. If source-write maintenance fails after the source commit, Midge's durable rollup-maintenance debt keeps the source authoritative and is retried from the source on startup. Recovery reparses the canonical aggregate SQL and publishes `Ready` only after a complete rebuild. A failed explicit refresh after replacement begins remains non-ready; the operator can retry `REFRESH ROLLUP` after resolving the storage or resource failure.

See [Production Readiness](production-readiness.md#rollup-and-retention-operator-contract) for operator diagnostics and the retained capacity boundary.
