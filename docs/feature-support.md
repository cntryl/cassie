SQL/query feature inventory

Category	Items
Query	SELECT, FROM, WHERE
Projection	*, explicit columns, aliases, expressions, scalar functions
Filtering	=, !=, <>, <, <=, >, >=, AND, OR, NOT
Nulls	IS NULL, IS NOT NULL
Lists	IN, NOT IN
Ranges	BETWEEN, NOT BETWEEN
Ordering	ORDER BY, ASC, DESC, NULLS FIRST, NULLS LAST, aliases
Pagination	LIMIT, OFFSET
Deduplication	DISTINCT, DISTINCT ON
Aggregates	count, sum, avg, min, max
Grouping	GROUP BY, HAVING
Joins	INNER JOIN, LEFT JOIN, RIGHT JOIN, FULL OUTER JOIN, CROSS JOIN
Semi/anti joins	EXISTS, NOT EXISTS
Lateral	LATERAL, CROSS APPLY, OUTER APPLY
Subqueries	scalar subqueries, FROM (...), WHERE (...), correlated subqueries
CTEs	WITH, WITH RECURSIVE
Set operations	UNION, UNION ALL, INTERSECT, EXCEPT
Window functions	row_number, rank, dense_rank, lag, lead, first_value, last_value, frames
DML	INSERT, UPDATE, DELETE, RETURNING
DDL	CREATE TABLE, ALTER TABLE, DROP TABLE, CREATE SCHEMA, DROP SCHEMA, CREATE INDEX, DROP INDEX
Transactions	BEGIN, COMMIT, ROLLBACK, savepoints
Views	CREATE VIEW, DROP VIEW, nested views
Functions	scalar functions, user-defined functions
Procedures	CREATE PROCEDURE, CALL
Types	text, bool, integers, floats, decimal, timestamp, uuid, json, arrays, vector
Casts	CAST(x AS type), x::type

Cassie-specific query features

Category	Items
Full-text	search(field, query), search_score(field, query), snippet(field, query)
Vector	vector_score, vector_distance, cosine_distance, dot_product, l2_distance
pgvector syntax	<=>, <->, <#>, vector(n)
Hybrid	hybrid_score(text_score, vector_score)
Embeddings	provider, model, dimensions, metric validation
Projections	projection metadata, schema version, offset, lag, rebuild state
Time-series	bucket, rollup, retention, range queries
Merkle	row hash, range hash, projection root, diff

Index inventory

Category	Items
Primary	primary key index
Secondary	single-column index
Composite	multi-column index
Unique	unique index / unique constraint
Covering	INCLUDE (...)
Partial	CREATE INDEX ... WHERE ...
Expression	CREATE INDEX ON table (lower(email))
Full-text	inverted index
Vector	brute force, HNSW, IVFFlat
Hybrid	text candidate + vector rerank metadata
Column-store	columnar index / column batches
Time-series	time-bucket index
Merkle	integrity index

Constraint inventory

Category	Items
Identity	PRIMARY KEY
Nullability	NOT NULL
Uniqueness	UNIQUE
Validation	CHECK
Defaults	DEFAULT
References	FOREIGN KEY
Generated	generated columns

Planner/executor inventory

Category	Items
Frontend	lexer, parser, AST
Binding	name resolution, type resolution, function resolution, parameter binding
Plans	logical plan, physical plan
Optimization	predicate pushdown, projection pruning, limit pushdown, index selection
Sorting	full sort, partial sort, top-k
Joins	nested-loop, hash join, merge join, semi join, anti join
Aggregation	hash aggregate, sort aggregate
Distinct	hash distinct, sort distinct
Execution	row executor, batch/vectorized executor
Caching	plan cache, function cache, prepared statement cache
Adaptive	runtime stats, cardinality feedback, adaptive candidate sizing
Parallel	parallel scan, parallel scoring, parallel aggregation

Protocol/API inventory

Category	Items
PostgreSQL wire	startup, auth, simple query, extended query, parse, bind, describe, execute, sync, close
Pgwire results	row description, data row, command complete, error response, ready for query
Pgwire compatibility	prepared statements, portals, text/binary formats, catalog introspection
HTTP	SQL query, search query, vector query, hybrid query, document APIs, admin APIs
Observability	EXPLAIN, EXPLAIN ANALYZE, query stats, operator stats, index used, rows scanned
Metrics	latency, throughput, errors, cache hit rate, projection lag