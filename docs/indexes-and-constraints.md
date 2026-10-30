# Indexes and Constraints

Cassie keeps stored row and field values as the source of truth and uses indexes, constraints, and analytical overlays to preserve correctness and accelerate reads. Midge remains the direct storage layer for all persisted index and constraint metadata.

## Support Summary

Status is owned by [Feature Support](feature-support.md); this summary follows that canonical owner.

| Area | Status | Guarantee |
| --- | --- | --- |
| Primary key indexes | Experimental | Identity, point lookup, duplicate rejection, catalog metadata |
| Unique constraints | Experimental | Duplicate rejection over supported keys |
| NOT NULL | Experimental | Write-time validation |
| CHECK | Experimental | Deterministic scalar validation |
| DEFAULT | Experimental | Default value application on supported write paths |
| Foreign keys | Experimental | Projection validation with documented limits |
| Generated columns | Experimental | Generated value support with documented limits |
| Secondary scalar indexes | Experimental | Equality and range acceleration |
| Composite indexes | Experimental | Left-prefix planning for supported predicates |
| Covering indexes | Experimental | INCLUDE metadata and covered-read planning |
| Partial indexes | Experimental | Exact normalized predicate matching |
| Expression indexes | Experimental | Deterministic expression matching |
| Full-text indexes | Stable | Cassie inverted index and BM25 support |
| Vector indexes | Stable | Declared exact, HNSW and IVFFlat candidate/reranking contracts; no pgvector extension ABI promise |
| Graph adjacency sidecars | Experimental | Outbound/inbound edge traversal for `CREATE GRAPH` backing tables |
| Time-series indexes | Stable | Fixed positive minute/hour/day UTC widths, bucket-native membership, authoritative fallback, diagnostics and restart-safe metadata |
| Column-batch indexes | Stable | Covered scans, segment pruning, aggregate acceleration |
| Retention policies | Experimental | Explicit timestamp-based cleanup with catalog and metrics diagnostics |

## Design Rules

- Row blobs remain the correctness fallback.
- Indexes accelerate reads; they do not become a second storage abstraction.
- Metadata must persist, hydrate after restart, and clean up on drop/rename/rebuild paths.
- Planner-selected indexes must preserve query semantics.
- Unsupported or unsafe acceleration paths must fall back deterministically.
- User-visible planner choices should appear in EXPLAIN or metrics when they affect performance.

## Constraint metadata and reservation identity

Constraint metadata, unique-owner publication and cleanup use the canonical
relation identity: unqualified names resolve in the configured default database's
`public` schema; schema-qualified names use that database; database-qualified
names retain their explicit database and schema. Equivalent spellings address
one constraint set and one reservation owner namespace.

Both `Midge::save_constraints` and
`Midge::save_constraints_with_unique_reservations` validate existing rows and
backfill newly single-column UNIQUE/PRIMARY KEY owners before publishing the
constraint metadata. Invalid existing duplicates reject publication with the
declared constraint identity. Constraint removal uses
`save_constraints_and_release_unique_reservations`; its durable cleanup is
replayed on restart. Publication’s metadata-only branch uses a private writer to terminate public-save
delegation. Empty cleanup uses the public validation/backfill path; removal
metadata and cleanup intent are written atomically. The existing gate order and
recovery machinery remain in use.

Cassie is predeployment. This repair selects canonical keys directly; no legacy
record migration or compatibility reader is required. The application validation
fallback remains in place, and broader constraint ingress/integration acceptance
remains tracked by #769. The namespace/backing repair is tracked by #953;
executed results are recorded in [Validation](testing/validation.md). Support
classification remains Experimental.

## Scalar and Composite Indexes

Scalar and composite indexes support common equality and range filters.

```sql
CREATE INDEX ON applications (tenant_id, status);
CREATE INDEX ON applications (tenant_id, status, created_at);
```

Supported predicate shapes include:

- Equality on indexed fields.
- Range predicates on supported scalar fields.
- Left-prefix composite lookup.
- Combined predicates extracted from simple conjunctions.
- JSON scalar indexes and `UNIQUE` reservations support objects and arrays using deterministic JSON serialization with recursively sorted object keys. Native SQL `ARRAY` values also work in primary, unique, and secondary scalar indexes. Range predicates and index-ordered reads on JSON fields fall back to row scans because integer and float key tags are separate even when the executor compares those values numerically.
- Scalar btree definitions accept `ASC`/`DESC`, `NULLS FIRST`/`LAST`, `NULLS [NOT] DISTINCT` for unique indexes, and the selected `text_pattern_ops` class for text-like fields. The default unique-index policy treats NULL keys as distinct; `NULLS NOT DISTINCT` persists that policy, rejects duplicate NULL keys, and is reported by `pg_index.indnullsnotdistinct` and `pg_get_indexdef`.
- Column- and table-level `UNIQUE NULLS [NOT] DISTINCT` constraints use the same NULL policy and create a scalar backing index, including for composite keys. `pg_constraint.conindid` reports the backing index using Cassie's text display-label catalog representation. A constraint-owned index cannot be dropped directly (`2BP01`); dropping the constraint removes its index. Evidence includes [`should_enforce_composite_unique_nulls_not_distinct_constraint`](../tests/compatibility/lifecycle/constraints/constraint_control.rs) and [`should_preserve_unique_null_policy_after_restart`](../tests/storage_indexes/indexes/scalar_indexes/restart.rs).
- Table-level UNIQUE constraints in `CREATE TABLE` and `ALTER TABLE` accept `INCLUDE` payload columns and the selected btree storage parameters `fillfactor` and `deduplicate_items`; INCLUDE fields do not participate in uniqueness, and duplicate, key-overlapping, or unknown INCLUDE fields are rejected. `ALTER TABLE ... ADD [CONSTRAINT name] UNIQUE USING INDEX name` adopts an eligible unique btree index with default key ordering and no expression keys or partial predicate. A named constraint renames the adopted index while preserving its catalog identity, NULL policy, and unique reservation namespace; dropping the constraint removes it and its reservations. Evidence: [`should_preserve_include_columns_on_a_unique_constraint_index`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_normalize_include_field_spelling_for_altered_unique_constraints`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_attach_an_existing_unique_index_as_a_table_constraint`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_keep_composite_unique_enforcement_after_constraint_adoption`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_preserve_nulls_not_distinct_when_adopting_unique_index`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), and [`should_protect_constraint_owned_index_until_constraint_drop`](../tests/compatibility/lifecycle/constraints/constraint/semantics.rs).
- Scalar btree indexes accept `fillfactor` values from 10 through 100 and `deduplicate_items` values `on` or `off` in `CREATE INDEX ... WITH (...)` and `ALTER INDEX ... SET/RESET (...)`. These values are durable index metadata reported by `pg_class.reloptions` and `pg_get_indexdef`; they do not change Midge index layout or query behavior. Other btree parameters and SQL GIN, GiST, BRIN, and SP-GiST methods or options are unsupported; attempted CREATE statements reject without publishing an index. GIN pending-list cleanup, BRIN range maintenance, PostgreSQL `VACUUM`, and `ANALYZE` are unsupported. `ALTER INDEX ... ALTER COLUMN ... SET STATISTICS` is unsupported while truthful index-attribute rows and statistics targets remain pending under [#817](https://github.com/cntryl/cassie/issues/817), after its [#777](https://github.com/cntryl/cassie/issues/777) prerequisite.
- A modifier-bearing key list that mixes field and expression keys is rejected with SQLSTATE `0A000`; the current scalar index metadata does not preserve that mixed key order.
- For compatibility with the existing Cassie catalog surface, `pg_index.indexrelid` and `indrelid` are text display labels; `indexrelid_oid` and `indrelid_oid` provide their numeric OIDs. This does not claim PostgreSQL's native `pg_index` column types.

### Rebuilding scalar indexes

`REINDEX INDEX name` rebuilds one existing scalar index from stored rows.
`REINDEX TABLE name` rebuilds every scalar index on the table. These forms run
outside active transactions; REINDEX is not part of Cassie's selected transactional
DDL set. Both forms
accept an optional `(VERBOSE FALSE, CONCURRENTLY FALSE)` option list; concurrent
rebuilds and verbose output are unsupported. Schema, database, and system
targets are also outside the selected support surface. `REINDEX TABLE` is
rejected when the table has a non-scalar index, so it never silently skips an
index type. Rebuilds take the collection writer gate and replace each index's
stored key prefix in a Midge data transaction. Focused coverage is in
[`should_rebuild_an_existing_index_with_reindex`](../tests/compatibility/lifecycle/indexes/index.rs),
[`should_rebuild_all_indexes_for_a_table`](../tests/compatibility/lifecycle/indexes/index.rs),
[`should_preserve_partial_index_results_after_reindex_with_null_keys`](../tests/compatibility/lifecycle/indexes/reindex_options.rs),
[`should_reindex_with_explicit_nonconcurrent_options_while_preserving_rows`](../tests/compatibility/lifecycle/indexes/reindex_options.rs),
[`should_reject_reindex_inside_an_explicit_transaction`](../tests/compatibility/lifecycle/indexes/reindex_options.rs),
[`should_serialize_scalar_index_rebuild_with_collection_writes`](../src/midge/adapter/scalar_indexes.rs),
and [`should_not_rebuild_scalar_index_from_metadata_removed_before_gate_acquisition`](../src/midge/adapter/scalar_indexes.rs).

NULL and non-finite values:

- Rows whose index key contains NULL are omitted from ordinary scalar index entries. Unique indexes declared `NULLS NOT DISTINCT` encode NULL keys for uniqueness enforcement. The planner uses a scalar index when every key column is compared with a value in the filter or is declared `NOT NULL` (or is part of the primary key).
- One nullable, unconstrained key is allowed when it is the leading `ORDER BY` key, NULLs sort last (`ASC` by default), and the query has a `LIMIT`. For example, `ORDER BY score LIMIT 10` or `WHERE status = $1 ORDER BY score LIMIT 10` on `(status, score)`, including an expression key such as `ORDER BY lower(title) LIMIT 10`. If the index yields fewer than `LIMIT + OFFSET` rows, execution falls back to the scan plus top-k sort so NULL-keyed rows are still returned.
- Other shapes decline the index: NULLs sorting first (`DESC` by default), no `LIMIT`, or a nullable key that is not the leading order key (for example `WHERE x > 1` on `(x, y)`).
- The full-text scalar prefilter and indexed join probes use a composite index only when its trailing key columns are `NOT NULL`.
- Comparisons with a NULL bind parameter match no rows.
- NaN and infinite float parameters are not index-encodable; range predicates that bind them fall back to the row scan, which orders NaN and infinities above every finite value. Writes reject non-finite float values instead of storing NULL.

Whole-number `FLOAT` keys:

- Integer-shaped values written to a `FLOAT` column (for example `INSERT ... VALUES (5)` or an `INT8` bind parameter) are indexed as floats, matching index backfill and query bounds. Unique reservation keys for `FLOAT` `UNIQUE` and primary key columns, and for unique scalar indexes, use the same float encoding.
- Scalar indexes on `FLOAT` columns that received such writes after the index was created, on a build before this rule, can hold entries that range and equality lookups miss. Drop and recreate those indexes to rebuild them from row blobs. No automatic migration runs.
- Rows written that way on an earlier build also hold integer-encoded unique reservation keys. A new write of the same whole-number value uses the float encoding and does not collide with them, and the write-time uniqueness scan compares stored JSON values exactly (`5` and `5.0` differ), so it does not close that gap. Rewrite affected rows (for example `UPDATE t SET price = price`) so they store float-encoded reservations before relying on uniqueness for those values.

Out of scope unless separately documented:

- Arbitrary skip-column lookup.
- Operator-class behavior beyond metadata-only `text_pattern_ops` reporting, and collation-specific index behavior.
- Planner hints.

## Covering Indexes

Covering indexes use INCLUDE metadata to avoid row fetches when a query is fully covered.

```sql
CREATE INDEX ON applications (tenant_id, status)
INCLUDE (id, created_at, applicant_name);
```

Expected behavior:

- Planner detects when projected fields are fully covered.
- Executor can return covered values without fetching row blobs.
- Row fetch remains the fallback when any required field is not covered.
- EXPLAIN and metrics should make covered-read choices visible.

## Partial Indexes

Partial indexes reduce index size for filtered read-model views.

```sql
CREATE INDEX ON applications (tenant_id, created_at)
WHERE status = 'approved';
```

Current guarantee:

- Predicate metadata is persisted and hydrated.
- Writes add entries only when the predicate matches.
- Planner selects the index when the query predicate has the same normalized expression representation.

Current limitation:

- Broader PostgreSQL-style predicate implication is not guaranteed.

## Expression Indexes

Expression indexes support deterministic expression lookup.

```sql
CREATE INDEX ON users (lower(email));
```

Current guarantee:

- Supported expression metadata is persisted and hydrated.
- Planner matching uses Cassie expression normalization.
- `pg_get_indexdef` formats accepted stored expression nodes as SQL; the cast and conditional-expression forms are covered by [`should_render_supported_expression_index_definitions`](../tests/compatibility/lifecycle/indexes/expression_index_definition.rs).
- Exact equality predicates on deterministic expression keys are storage-backed scalar index seeks.
- Untargeted `ON CONFLICT` resolves unique expression-index conflicts through committed reservation keys and checks only staged transaction changes by iteration.
- Projection fields that are not stored in the index are fetched from row blobs.
- Non-equivalent or unsupported expressions fall back to non-expression paths.

Current limitation:

- Full PostgreSQL expression equivalence, collation, and operator-class behavior is not guaranteed.
- Expression range scans and expression ORDER BY proofs are not claimed.

## Full-Text Indexes

Full-text indexes support named analyzer options.

```sql
CREATE INDEX ON documents USING fulltext (body)
WITH (analyzer = standard, stop_words = none, accent_folding = true);
```

Supported options:

- `analyzer = standard`
- `analyzer = simple`
- `tokenizer = standard`
- `tokenizer = whitespace`
- `case_folding = true|false`
- `stop_words = english|none`
- `stemming = none`
- `accent_folding = true|false`

Analyzer options are persisted and used by indexing, `search()`, `search_score()`, snippets, BM25 statistics, rebuild scans, and cached full-text scoring metadata.

## Vector Indexes

Vector indexes support Cassie vector search and pgvector-style operator surfaces.

```sql
CREATE INDEX ON documents USING vector (embedding)
WITH (source_field = body, metric = l2, index_type = hnsw, m = 16, ef_construction = 64, ef_search = 40);
```

Supported option families:

- `index_type = bruteforce|hnsw|ivfflat`
- `metric = cosine|l2|dot`
- HNSW tuning options such as `m`, `ef_construction`, and `ef_search`.
- IVFFlat options such as `lists`, `probes`, `training_sample_size`, and `training_seed`.

SQL and REST vector-index creation use the same option normalization and validation. REST index
responses include the normalized `index_type`.

Guarantees:

- Candidate paths must verify exact score or distance ordering before returning results when required by the selected algorithm.
- Metric and dimension mismatches must produce deterministic errors or fallback behavior.
- EXPLAIN and metrics should expose index use and fallback reasons where relevant.

Current HNSW support:

- Cassie persists and hydrates deterministic HNSW graph state in a vector-index state record
  alongside immutable index metadata. Changed collection batches commit normalized-vector sidecars
  and graph state together.
- Compatible unfiltered top-k vector-distance searches use the graph candidate path, then fetch row
  vectors and re-rank exactly before returning SQL or REST-visible rows.
- Document writes and deletes refresh graph state for affected collections under a per-collection
  write gate. HNSW is Stable within the declared candidate-selection and source-row reranking
  contract. Missing, stale, incompatible, empty or unsupported graph shapes fall back to the
  exact row/vector path with a deterministic fallback reason. Exact candidate reranking does
  not promise globally exact ANN top-k; recall evidence remains a separate contract.

Current IVFFlat support:

- Cassie persists and hydrates IVFFlat metadata/options plus deterministic training state.
  Changed collection batches commit normalized-vector sidecars and training state together.
- SQL and REST top-k searches over compatible L2 vector-distance shapes probe trained lists, then
  fetch row vectors and re-rank exactly before returning visible rows.
- Document writes and deletes refresh IVFFlat training state for affected collections. IVFFlat
  is Stable within the declared candidate-selection and source-row reranking contract;
  unsupported shapes fall back to the exact row/vector path. Candidate exhaustion, resource
  bounds and recall retain their [Performance Contracts](performance-contracts.md) requirements.

## Graph Adjacency Sidecars

`CREATE GRAPH` creates normal node and edge backing tables plus persisted graph metadata.
Edge writes maintain outbound and inbound adjacency sidecars keyed by typed source and target ids.

```sql
CREATE GRAPH social (NODES (label TEXT), EDGES (source TEXT));
SELECT node_id
FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 10);
```

Current guarantee:

- Node identity is `(node_type, node_id)`, with ASCII case-insensitive node types and case-sensitive node ids.
- Edge rows use typed source/target ids, `edge_type`, and non-negative numeric `weight`.
- `graph_neighbors`, `graph_expand`, and `graph_shortest_path` are SQL table functions.
- `graph_shortest_path` returns up to `max_paths` lowest-cost simple paths within `max_depth`, ordered by total weight. A path cannot visit the same typed node twice; traversal is limited by the query's timeout and memory budget.
- Exact simple-path enumeration can grow exponentially. Exceeding the timeout or memory budget fails the query instead of returning a truncated set of paths.
- Adjacency sidecars are accelerators; edge row blobs remain authoritative.
- Dropping a graph node or edge backing collection removes its outbound/inbound adjacency sidecars,
  including replay after an interrupted schema commit.
- EXPLAIN reports `access_path=graph_adjacency`, and `/metrics` reports graph traversal counters.

Current limitation:

- Graph support is Cassie-specific and experimental; Cypher, Gremlin, distributed traversal, and graph mutation languages are not supported.

## Time-Series Indexes

Time-series indexes declare a timestamp field for range-oriented planning.

```sql
CREATE INDEX idx_events_created_at
ON events USING time_series (created_at)
WITH (bucket_width = '1 hour', partition_by = tenant_id);
```

Current guarantee:

- Parser, binder, catalog metadata, restart hydration, and EXPLAIN planner selection are supported for timestamp range predicates.
- EXPLAIN includes selected bucket width, partition fields, and range-filter diagnostics for selected time-series indexes.
- Bucket-native membership is persisted in the owning per-database Midge family under the `cassie-midge-layout-v1-0.1.0` time-series family. The canonical format and access-path requirements live in [Performance Contracts](performance-contracts.md).
- Bucket-native hits load authoritative row blobs and then run the normal filter, sort, and projection path, so bucket metadata is a candidate accelerator rather than the source of truth.
- Row-backed time-series range execution remains the correctness fallback when planner proof selects a time-series index but bucket-native data is missing, corrupt, or unsupported.
- Runtime metrics expose bucket-native hits, selected scans, rows, scanned buckets, skipped buckets, last index, and fallback reasons.
- Insert/update/delete/restart correctness is preserved because row blobs remain authoritative.
- Retention enforcement uses normal document deletion, refreshes source rollups, and marks dependent materialized projections stale for re-verification.
- The indexed field must be a timestamp, and unsupported unique, partial, expression, or INCLUDE forms are rejected.

Current limitation:

- Time-series index DDL accepts positive integer minute(s), hour(s) or day(s) bucket widths
  interpreted as fixed UTC durations. Second, week, month, calendar-aware, local-time,
  daylight-saving, zero, negative and malformed widths are rejected; they are not accepted
  as index metadata for fallback. Missing, stale, corrupt, interrupted or unsupported derived
  artifacts for an otherwise supported index fall back atomically to authoritative rows.
  Unsupported index DDL and supported-index artifact fallback are separate outcomes. This
  index-width restriction does not expand or narrow the scalar `time_bucket` or rollup contracts.

## Column-Batch Indexes

Column-batch indexes are optional analytical acceleration.

```sql
CREATE INDEX idx_applications_column
ON applications USING column (status, created_at, amount)
WITH (segment_size = 1024);
```

Supported behavior:

- Column batches are rebuilt from row blobs.
- Segment summaries prune covered equality, range, `IS NULL`, and `IS NOT NULL` predicates.
- Covered projected scans can avoid row fetches.
- Summary metadata can accelerate eligible unfiltered, non-grouped `count`, `sum`, `avg`, `min`, and `max`.
- Query execution falls back to row scans for correctness.
- EXPLAIN reports `column_batch_index=<name>` for eligible covered scans and `aggregate_acceleration=true` for eligible summary aggregates.
- Runtime metrics expose scan, fallback, byte, segment, decoded-column, and aggregate-acceleration counters.

## Retention Policies

Retention policies delete expired rows through explicit deterministic enforcement.

```sql
CREATE RETENTION POLICY events_retention
ON events USING event_at
RETAIN FOR '7 days';

ENFORCE RETENTION POLICY events_retention AT '2026-01-10T00:00:00Z';
```

Supported behavior:

- Policy metadata persists and hydrates after restart.
- `ALTER RETENTION POLICY ... RETAIN FOR ...` updates the retained duration.
- Enforcement deletes rows older than `AT - duration` and skips missing or invalid timestamps with diagnostics.
- Row blobs remain authoritative; deletion uses the normal document cleanup path so dependent index/vector/column state is refreshed.
- `pg_catalog.pg_retention_policies` and runtime metrics expose state, deletes, skips, and errors.

## Constraint Behavior

Constraints are correctness gates for projection state.

Required behavior:

- Validation runs before writes become visible.
- Validation failures must not partially mutate row blobs or indexes.
- Constraint metadata must persist and hydrate after restart.
- Constraint errors visible through pgwire should map to deterministic SQLSTATE-style responses.

Compatibility notes:

- Primary key, unique, not-null, check, and default behavior should stay PostgreSQL-like for supported syntax.
- Foreign key and generated-column behavior should be documented with explicit limits because Cassie is a projection/read-model database, not a full OLTP PostgreSQL replacement.

Multi-column UNIQUE constraints:

- `UNIQUE (a, b)` and `CONSTRAINT name UNIQUE (a, b)` reject a row only when the whole tuple `(a, b)` already exists. A row that shares only one of the columns is accepted, and a tuple that contains a NULL never conflicts. An unnamed constraint is named `<table>_<a>_<b>_key`, as in PostgreSQL.
- The constraint is enforced through a unique scalar index with the constraint's name, created by `CREATE TABLE` or `ALTER TABLE ... ADD CONSTRAINT`. Adding the constraint fails if existing rows repeat a tuple. `DROP CONSTRAINT` drops the index. `DROP INDEX` on the backing index is refused. `DROP COLUMN` on one of its columns drops the whole constraint.
- `ON CONFLICT (a, b)` uses the constraint as its arbiter, with the target columns in any order.
- A column can belong to only one UNIQUE constraint when either of the constraints spans several columns (for example `a INT UNIQUE` together with `UNIQUE (a, b)`, or `UNIQUE (a, b)` together with `UNIQUE (b, c)`). Such DDL is rejected rather than applied partially. A multi-column `PRIMARY KEY` is not supported.

Foreign key DDL lifecycle:

- Constraint column lists match declared columns without regard to ASCII case and are stored with the declared spelling.
- `ALTER TABLE ... ADD CONSTRAINT ... REFERENCES` resolves the referenced relation through `search_path` exactly as `CREATE TABLE` does.
- `ALTER TABLE ... RENAME TO` and `ALTER TABLE ... RENAME COLUMN` carry every foreign key that references the renamed table or column.
- `ALTER TABLE ... DROP COLUMN` is refused while a foreign key on another column references the dropped column; a foreign key declared on the dropped column is removed with it.
- `DROP TABLE` is refused while a foreign key on another table references the table. `DROP TABLE ... CASCADE` removes those inbound foreign-key constraints while retaining the referencing tables and their rows. Rollback restores the constraint, and a committed cascade remains effective after local-storage restart; a table's self-referencing foreign key is dropped with its table. Evidence: [`should_restore_inbound_foreign_key_after_table_cascade_rollback`](../tests/sql_mutations/constraints/foreign_keys/foreign_key_ddl_lifecycle.rs) and [`should_preserve_inbound_foreign_key_removal_after_table_cascade_restart`](../tests/sql_mutations/constraints/foreign_keys/foreign_key_ddl_lifecycle.rs).
- Foreign key violations name the constraint as declared, or as generated when it is unnamed, matching `information_schema.table_constraints`.

## Benchmark Expectations

Performance-sensitive index work should include benchmark or metrics evidence before being called production-ready.

| Area | Good | Excellent |
| --- | ---: | ---: |
| Primary-key point lookup | `<200 us` | `<100 us` |
| Indexed filter 10k | `<700 us` | `<400 us` |
| Range query 10k | `<800 us` | `<600 us` |
| Covering indexed query 10k | `<500 us` | `<300 us` |
| Upsert with index maintenance | `<750 us` | `<500 us` |
| Delete with index maintenance | `<750 us` | `<500 us` |
| Index rebuild 10k | `<8 ms` | `<5 ms` |
