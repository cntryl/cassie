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
| Column-batch indexes | Stable | Covered scans, segment pruning, aggregate acceleration |

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
- Non-equivalent or unsupported expressions fall back to non-expression paths.

Current limitation:

- Full PostgreSQL expression equivalence, collation, and operator-class behavior is not guaranteed.

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

- `index_type = bruteforce|hnsw`
- `metric = cosine|l2|dot`
- HNSW tuning options such as `m`, `ef_construction`, and `ef_search`.

Guarantees:

- Candidate paths must verify exact score or distance ordering before returning results when required by the selected algorithm.
- Metric and dimension mismatches must produce deterministic errors or fallback behavior.
- EXPLAIN and metrics should expose index use and fallback reasons where relevant.

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
