# Indexes and Constraints

Cassie keeps row blobs as the source of truth and uses indexes, constraints, and analytical overlays to preserve correctness and accelerate reads. Midge remains the direct storage layer for all persisted index and constraint metadata.

## Support Summary

| Area | Status | Guarantee |
| --- | --- | --- |
| Primary key indexes | Stable | Identity, point lookup, duplicate rejection, catalog metadata |
| Unique constraints | Stable | Duplicate rejection over supported keys |
| NOT NULL | Stable | Write-time validation |
| CHECK | Stable | Deterministic scalar validation |
| DEFAULT | Stable | Default value application on supported write paths |
| Foreign keys | Stable/Experimental | Projection validation with documented limits |
| Generated columns | Stable/Experimental | Generated value support with documented limits |
| Secondary scalar indexes | Stable | Equality and range acceleration |
| Composite indexes | Stable | Left-prefix planning for supported predicates |
| Covering indexes | Stable | INCLUDE metadata and covered-read planning |
| Partial indexes | Experimental | Exact normalized predicate matching |
| Expression indexes | Experimental | Deterministic expression matching |
| Full-text indexes | Stable | Cassie inverted index and BM25 support |
| Vector indexes | Stable/Experimental | Brute force, HNSW, and IVFFlat surfaces by support level |
| Time-series indexes | Experimental | Timestamp range planning, row-backed scans, bucket diagnostics, restart-safe metadata |
| Column-batch indexes | Stable | Covered scans, segment pruning, aggregate acceleration |
| Retention policies | Experimental | Explicit timestamp-based cleanup with catalog and metrics diagnostics |

## Design Rules

- Row blobs remain the correctness fallback.
- Indexes accelerate reads; they do not become a second storage abstraction.
- Metadata must persist, hydrate after restart, and clean up on drop/rename/rebuild paths.
- Planner-selected indexes must preserve query semantics.
- Unsupported or unsafe acceleration paths must fall back deterministically.
- User-visible planner choices should appear in EXPLAIN or metrics when they affect performance.

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

Out of scope unless separately documented:

- Arbitrary skip-column lookup.
- PostgreSQL operator classes and collation-specific index behavior.
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
- Exact equality predicates on deterministic expression keys are storage-backed scalar index seeks.
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
- IVFFlat metadata options such as `lists`, `probes`, `training_sample_size`, and `training_seed`.

Guarantees:

- Candidate paths must verify exact score or distance ordering before returning results when required by the selected algorithm.
- Metric and dimension mismatches must produce deterministic errors or fallback behavior.
- EXPLAIN and metrics should expose index use and fallback reasons where relevant.

Current IVFFlat support:

- Cassie persists and hydrates IVFFlat metadata/options plus deterministic training state.
- IVFFlat top-k queries over compatible L2 vector-distance shapes probe trained lists, then fetch row vectors and re-rank exactly before returning SQL-visible rows.
- Document writes and deletes refresh IVFFlat training state for affected collections. IVFFlat remains experimental; unsupported shapes fall back to the exact row/vector path.

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
- Row-backed time-series range execution is supported when planner proof selects a time-series index.
- Runtime metrics expose selected scans, rows, scanned buckets, skipped buckets, last index, and fallback reasons.
- Insert/update/delete/restart correctness is preserved because row blobs remain authoritative.
- Retention enforcement uses normal document deletion, refreshes source rollups, and marks dependent materialized projections stale for re-verification.
- The indexed field must be a timestamp, and unsupported unique, partial, expression, or INCLUDE forms are rejected.

Current limitation:

- Persisted bucket membership and bucket-native storage scans remain planned depth work. The MVP path computes bucket diagnostics from authoritative rows instead of introducing a second storage abstraction.

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
