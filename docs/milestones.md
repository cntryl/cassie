Cassie Roadmap

V1 — Projection Query Engine

Storage

* row blob storage
* schema versioning
* field-id based encoding
* sparse rows
* projection metadata

SQL

* SELECT
* FROM
* WHERE
* ORDER BY
* LIMIT
* OFFSET
* DISTINCT
* GROUP BY
* HAVING
* EXISTS
* NOT EXISTS
* INNER JOIN
* LEFT JOIN
* UNION
* UNION ALL
* CTEs
* basic aggregates
* scalar functions
* casts

DML

* INSERT
* UPDATE
* DELETE
* RETURNING

Constraints

* PRIMARY KEY
* NOT NULL
* UNIQUE
* CHECK
* DEFAULT

Indexes

* primary key index
* secondary indexes
* composite indexes
* unique indexes

Search

* inverted index
* BM25 scoring
* snippets
* search()
* search_score()

Vector

* vector fields
* brute-force search
* cosine distance
* dot product
* L2 distance
* pgvector operators
* embedding validation

Hybrid

* hybrid_score()
* text candidate generation
* vector reranking

Execution

* logical plans
* physical plans
* batch execution
* plan cache
* prepared statements

Protocols

* PostgreSQL wire protocol
* HTTP API

Observability

* EXPLAIN
* query statistics
* operator statistics
* metrics

⸻

V2 — Query Performance

Planner

* predicate pushdown
* projection pruning
* limit pushdown
* index-aware planning
* top-k optimization

Joins

* hash joins
* semi joins
* anti joins

Search

* posting-list optimizations
* precomputed scoring metadata
* top-k search execution

Vector

* SIMD distance calculations
* normalized vector storage
* metadata prefilters

Caching

* function cache
* execution cache
* runtime statistics

Adaptive

* cardinality tracking
* runtime feedback
* adaptive candidate sizing

Indexes

* covering indexes
* INCLUDE columns

Execution

* zero-copy value access
* buffer reuse
* allocation reduction

⸻

V3 — Advanced Query Features

SQL

* recursive CTEs
* INTERSECT
* EXCEPT
* RIGHT JOIN
* FULL OUTER JOIN

Window Functions

* row_number
* rank
* dense_rank
* lag
* lead
* first_value
* last_value

Indexes

* partial indexes
* expression indexes

Search

* advanced analyzers
* custom tokenizers

Vector

* HNSW indexes

Execution

* parallel scans
* parallel scoring
* parallel aggregation

Observability

* EXPLAIN ANALYZE
* runtime plan diagnostics

⸻

V4 — Analytical Overlay

Column Store Indexes

* column batches
* compressed column segments
* aggregate acceleration
* scan acceleration

Time Series

* bucket functions
* rollups
* retention policies
* time-series indexes

Materialization

* materialized projections
* projection versioning
* projection swaps

Adaptive Planning

* cost-informed planning
* operator selection feedback
* index performance feedback

Vector

* IVFFlat indexes

⸻

V5 — Verification & Advanced Execution

Merkle Overlay

* row hashes
* range hashes
* projection Merkle roots
* projection diffing
* rebuild verification

Column Tables

* full column-store tables
* column-native execution paths
* hybrid row/column planning

Execution

* merge joins
* advanced parallel execution
* vectorized aggregation
* vectorized joins

Query Intelligence

* advanced cardinality estimation
* adaptive execution plans
* runtime operator switching

Distributed Read Models

* projection comparison
* projection integrity verification
* multi-instance consistency checks

Advanced Analytics

* analytical projections
* large-scale aggregations
* mixed search/vector/analytical execution