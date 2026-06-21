# Feature Ownership

Ownership in Cassie follows subsystem boundaries. A feature can span multiple subsystems, but one area should own the feature record and close-out.

## Ownership Map

| Feature Area | Primary Owner | Supporting Areas |
| --- | --- | --- |
| SQL parsing and AST | `src/sql/parser*`, `src/sql/ast.rs` | Binder, tests/parser_* |
| Binding and validation | `src/sql/binder*` | Catalog, planner |
| Logical and physical planning | `src/planner/` | Executor, runtime metrics |
| Query execution | `src/executor/` | Planner, midge, runtime |
| DML and DDL execution | `src/executor/execution/dml*.rs` | SQL binder, catalog, midge |
| Storage and metadata | `src/midge/` | Catalog, executor |
| Catalog objects | `src/catalog/` | SQL binder, executor, pgwire probes |
| Full-text search | `src/search/`, scored executor paths | Planner, runtime metrics |
| Vector search | `src/vector/`, scored executor paths | Embeddings, planner |
| Hybrid scoring | `src/hybrid/`, scored executor paths | Search, vector |
| Embeddings | `src/embeddings/`, `src/app/embeddings.rs` | REST, vector metadata |
| Column batches | `src/midge/adapter/column_batches.rs` | Planner, executor, metrics |
| Rollups and time series | Catalog, parser, executor rollup modules | Planner, runtime metrics |
| Pgwire protocol | `src/pgwire/` | App query execution, catalog probes |
| REST API | `src/rest/`, `src/app/` | Query, search, vector, embeddings |
| Metrics and feedback | `src/runtime*` | Planner, executor, pgwire |

## Review Boundaries

- SQL grammar changes need parser tests and binder validation coverage.
- Type or name-resolution changes need binder tests and at least one execution-level check.
- Planner changes need logical or physical plan tests plus result-level integration coverage when semantics can change.
- Executor changes need result-level tests and metrics/EXPLAIN updates when execution path selection is user-visible.
- Persisted metadata changes need restart hydration and cleanup tests.
- Pgwire changes need protocol lifecycle tests and SQLSTATE checks for error paths.
- REST changes need endpoint tests and compatibility notes when behavior overlaps pgwire.

## Ownership Defaults

- If a feature exposes SQL syntax, SQL owns the first parser/binder contract and the feature owner coordinates downstream changes.
- If a feature changes persisted state, Midge/catalog ownership must be explicit before implementation closes.
- If a feature changes client-visible behavior, pgwire or REST ownership must be included in the close-out.
- If a feature affects planning, EXPLAIN or metrics should show the important decision unless the behavior is invisible and semantics-preserving.
