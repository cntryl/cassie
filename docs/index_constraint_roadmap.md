# Cassie Index and Constraint Roadmap

Cassie V1 should keep the row blob as truth and add indexes as acceleration. Constraints are projection validation: they prevent bad read-model state, but should not try to model full OLTP semantics.

## Priority Order

| Feature | Priority | Why |
| --- | --- | --- |
| Primary key index | P0 | Enables point lookup, stable identity, upsert, and delete. |
| Unique constraints | P0 | Provides basic correctness for projection state. |
| `NOT NULL` / `CHECK` | P0 | Provides projection quality gates. |
| Secondary indexes | P0 | Accelerates equality and range filters. |
| Composite indexes | P1 | Covers common multi-field filters. |
| Covering indexes | P1 | Avoids row fetches for common projected queries. |
| Partial indexes | P2 | Keeps indexes smaller for filtered views. |
| Expression indexes | P2 | Supports derived lookups like `lower(email)`. |
| Column-store indexes | P2/P3 | Accelerates analytics, scans, and aggregates. |
| Foreign keys | P3 | Lower value for a projection/read-model database. |

## P0 Scope

### Primary Key Index

Add an explicit primary-key index as the identity path for point reads and writes.

Required behaviors:

- `PRIMARY KEY` syntax is parsed, bound, persisted, and hydrated.
- Inserts reject duplicate primary-key values.
- Upsert and delete can route through the primary-key index.
- Point lookup avoids full row scans.
- Catalog introspection exposes primary-key metadata.

Initial key shape:

```text
pk/{projection}/{collection}/{encoded_pk}/{row_id}
```

For single-column primary keys, `{encoded_pk}` is the normalized scalar value. Composite primary keys can reuse the composite index encoding once available.

### Unique Constraints

Unique constraints are projection correctness checks and should share index machinery with primary keys where possible.

Required behaviors:

- `UNIQUE` constraints are persisted and hydrated.
- Inserts and updates reject duplicate non-null values.
- Transaction-local writes participate in uniqueness checks.
- Catalog introspection reports unique constraints.

Initial key shape:

```text
uniq/{projection}/{collection}/{constraint}/{encoded_value}/{row_id}
```

### `NOT NULL` and `CHECK`

These already exist as validation concepts and should remain P0 quality gates for projection state.

Required behaviors:

- `NOT NULL` rejects missing or null projected values.
- `CHECK` evaluates deterministic scalar expressions before accepting writes.
- Validation runs for direct ingest, SQL insert, and SQL update.
- Validation failures must not partially mutate row blobs or indexes.

### Secondary Indexes

Start with scalar equality and range filters. This is the main execution-floor optimization after plan cache and executor scan work.

Example:

```sql
CREATE INDEX ON applications (tenant_id, status);
```

Good for:

```sql
WHERE tenant_id = $1 AND status = $2
```

Initial key shape:

```text
idx/{projection}/{index}/{tenant_id}/{status}/{row_id}
```

Required behaviors:

- Planner detects equality and range predicates that match available indexes.
- Executor scans index keys before fetching matching row blobs.
- Indexes are updated atomically with row writes.
- Index rebuild can derive entries from row blobs.
- Existing full scans remain the correctness fallback.

## P1 Scope

### Composite Indexes

Composite indexes should use left-prefix semantics.

Example:

```sql
CREATE INDEX ON applications (tenant_id, status, created_at);
```

Supported predicate shapes:

- `tenant_id = $1`
- `tenant_id = $1 AND status = $2`
- `tenant_id = $1 AND status = $2 AND created_at >= $3`

Non-goals for P1:

- Arbitrary skip-column lookup.
- Complex predicate reordering beyond simple conjunction extraction.

### Covering Indexes

Covering indexes are the highest-value P1 optimization because they can bypass row blob fetches for common projection queries.

Current V2 support parses, binds, persists, hydrates, and introspects scalar `INCLUDE` columns. Covered-query planning treats scalar index keys plus included columns as available projection fields; the versioned physical index-payload key/value shape below remains the target shape for deeper storage-level acceleration.

Example:

```sql
CREATE INDEX ON applications (tenant_id, status)
INCLUDE (id, created_at, applicant_name);
```

Query that should avoid row fetch:

```sql
SELECT id, created_at, applicant_name
FROM applications
WHERE tenant_id = $1 AND status = 'approved';
```

Key/value shape:

```text
idx/{projection}/{index}/{tenant_id}/{status}/{row_id}
  -> included columns
```

Required behaviors:

- Planner detects when projected fields are fully covered.
- Executor returns included values directly from the index payload.
- Row fetch remains available when a query needs non-covered fields.
- Covering payload encoding is versioned so index rebuilds can upgrade safely.

## P2 Scope

### Partial Indexes

Partial indexes should reduce index size for filtered read-model views.

Current V3 support persists and hydrates scalar partial-index predicates and selects them only when the query predicate has the same normalized expression representation. Broader implication checks remain a roadmap item.

Example:

```sql
CREATE INDEX ON applications (tenant_id, created_at)
WHERE status = 'approved';
```

Required behaviors:

- Predicate is persisted in index metadata.
- Writes only add entries when the predicate matches.
- Planner only selects the index when the query predicate implies the partial predicate.

### Expression Indexes

Expression indexes should support stable deterministic expressions only.

Example:

```sql
CREATE INDEX ON users (lower(email));
```

Required behaviors:

- Only immutable built-in scalar functions are allowed initially.
- Expression value is computed during writes and rebuilds.
- Planner matches equivalent expression predicates.

## P2/P3 Scope

### Column-Store Indexes

Column-store indexes are optional analytical acceleration, not default storage.

Example:

```sql
CREATE COLUMN INDEX ON applications (status, created_at, amount);
```

Physical shape:

```text
col/{projection}/{column}/{segment}
  -> compressed column batch
```

Good for:

```sql
SELECT status, count(*)
FROM applications
GROUP BY status;
```

Rules:

- Row blob remains the source of truth.
- Column indexes are rebuilt from row blobs.
- Column indexes accelerate scans and aggregates only when available.
- Query execution must fall back to row scans for correctness.

## P3 Scope

### Foreign Keys

Foreign keys are deferred for V1. Cassie is a projection/read-model database, so foreign keys are less important than primary keys, uniqueness, validation, and query acceleration.

If implemented later:

- Treat them as projection validation.
- Avoid cross-collection write coupling unless clearly required.
- Prefer optional async validation for bulk projection rebuilds.

## Implementation Sequence

1. Add primary-key metadata and index storage.
2. Route point lookup, upsert, and delete through primary-key indexes.
3. Back unique constraints with unique index entries.
4. Harden `NOT NULL` and `CHECK` validation across all write paths.
5. Add scalar secondary index metadata, write maintenance, and rebuild.
6. Add planner/executor support for indexed equality and range filters.
7. Add composite index left-prefix planning.
8. Add covering index syntax, metadata, payload encoding, and covered-query execution.
9. Add partial index metadata and predicate implication checks.
10. Add expression index metadata and deterministic expression evaluation.
11. Add optional column-store index segments for analytical scans.
12. Defer foreign keys until real workload evidence justifies them.

## Benchmark Gates

The P0/P1 index work should move these targets:

| Area | Good | Excellent |
| --- | ---: | ---: |
| Primary-key point lookup | `<200 us` | `<100 us` |
| Indexed filter 10k | `<700 us` | `<400 us` |
| Range query 10k | `<800 us` | `<600 us` |
| Covering indexed query 10k | `<500 us` | `<300 us` |
| Upsert with index maintenance | `<750 us` | `<500 us` |
| Delete with index maintenance | `<750 us` | `<500 us` |
| Index rebuild 10k | `<8 ms` | `<5 ms` |
