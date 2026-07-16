# Database Jargon Glossary

A reference for common database terms, organized by category. Each entry includes a short definition and, where useful, an example or related term.

---

## 1. Query Language Categories (the "-ML/-DL" family)

SQL is often split into sub-languages based on what the statements do.

| Term | Full Name | Purpose | Example Statements |
| --- | --- | --- | --- |
| DDL | Data Definition Language | Defines or alters the structure (schema) of database objects | `CREATE TABLE`, `ALTER TABLE`, `DROP TABLE`, `TRUNCATE` |
| DML | Data Manipulation Language | Reads and modifies the data inside tables | `INSERT`, `UPDATE`, `DELETE` (and sometimes `SELECT`) |
| DQL | Data Query Language | Retrieves data (sometimes considered a subset of DML) | `SELECT` |
| DCL | Data Control Language | Manages permissions and access | `GRANT`, `REVOKE` |
| TCL | Transaction Control Language | Manages transaction boundaries | `COMMIT`, `ROLLBACK`, `SAVEPOINT` |

Why it matters: these categories map to different concerns — DDL changes are often risky or locking, DML is your everyday app traffic, DCL is a security review topic, and TCL is about consistency guarantees.

---

## 2. Query Processing & Internals

| Term | Meaning |
| --- | --- |
| AST | Abstract Syntax Tree — the parsed, tree-shaped representation of a SQL query after the parser has broken it into grammatical components (SELECT clause, WHERE clause, JOIN, and so on) before optimization. Every query engine builds one before it can plan execution. |
| Query Planner / Optimizer | The component that takes the AST (or a logical plan derived from it) and decides how to execute the query — which indexes to use, join order, and so on. |
| Execution Plan / Query Plan | The concrete, ordered set of physical operations (scans, joins, sorts) the engine will run. Inspected via `EXPLAIN` or `EXPLAIN ANALYZE`. |
| Logical Plan vs. Physical Plan | Logical = what to do (relational algebra: filter, join, project). Physical = how to do it (nested-loop join vs. hash join, index scan vs. sequential scan). |
| Cardinality Estimation | The optimizer's guess at how many rows a step will produce, used to choose the cheapest plan. Bad estimates are a very common cause of slow queries. |
| Cost-Based Optimizer (CBO) | An optimizer that picks a plan by estimating and comparing the cost (I/O, CPU) of alternative plans, as opposed to a rule-based optimizer. |

---

## 3. Storage & Data Structures

| Term | Meaning |
| --- | --- |
| SST / SSTable | Sorted String Table — an immutable, sorted, on-disk file of key-value pairs. A core building block of LSM-tree storage engines (used in Cassandra, RocksDB, LevelDB, and HBase). Data is written to memory first, then flushed to disk as an SSTable. |
| LSM-Tree | Log-Structured Merge-Tree — a write-optimized storage structure. Writes go to an in-memory table (memtable) plus a write-ahead log, and are periodically flushed and merged (compacted) into SSTables on disk. Great for write-heavy workloads. |
| B-Tree / B+Tree | A balanced, sorted tree structure used by most traditional relational databases (Postgres, MySQL/InnoDB) for indexes. Optimized for reads and range scans; B+Trees keep data in leaf nodes with linked leaves for fast range queries. |
| WAL | Write-Ahead Log — a durability mechanism where changes are written to an append-only log before being applied to the actual data files. If the database crashes, it can replay the WAL to recover. Also called a redo log in some systems. |
| Compaction | The background process of merging and rewriting SSTables (or reclaiming space) to remove duplicates or deleted entries and improve read performance. |
| Page | The fixed-size unit of storage (commonly 4 KB or 8 KB) that a database reads or writes to disk at a time. |
| Heap (Heap Table/File) | The unordered storage structure holding actual row data, as opposed to an index that points into it. |
| Vacuum | (Postgres-specific term, but the concept is universal) Reclaiming space from deleted or updated rows and updating statistics for the planner. |

---

## 4. Transactions & Concurrency

| Term | Meaning |
| --- | --- |
| ACID | Atomicity, Consistency, Isolation, Durability — the classic guarantees of a transactional database. Atomic (all-or-nothing), consistent (valid state to valid state), isolated (concurrent transactions do not interfere), and durable (committed data survives crashes). |
| BASE | Basically Available, Soft state, Eventually consistent — the looser guarantee model often used by distributed or NoSQL systems that trade strict consistency for availability. |
| MVCC | Multi-Version Concurrency Control — a technique where the database keeps multiple versions of a row so readers do not block writers and vice versa. Used by Postgres, MySQL/InnoDB, and Oracle. |
| Isolation Levels | How much transactions are shielded from one another's in-progress changes: Read Uncommitted → Read Committed → Repeatable Read → Serializable (increasing strictness, decreasing concurrency). |
| Dirty Read / Phantom Read / Non-Repeatable Read | Anomalies that different isolation levels prevent — reading uncommitted data, getting different results on repeated reads, or seeing new rows appear mid-transaction. |
| Deadlock | Two or more transactions each waiting on a lock held by the other, forming a cycle with no resolution unless the database kills one of them. |
| Two-Phase Commit (2PC) | A protocol for committing a transaction atomically across multiple nodes or databases: a prepare phase followed by a commit phase. |
| Optimistic vs. Pessimistic Locking | Pessimistic: lock the row before modifying it. Optimistic: do not lock, but check a version or timestamp at commit time and fail if it changed. |

---

## 5. Distributed Systems & Scaling

| Term | Meaning |
| --- | --- |
| CAP Theorem | You can only fully guarantee two of three in a distributed system: Consistency, Availability, and Partition tolerance. Since network partitions are a fact of life, real systems choose between consistency and availability during a partition. |
| Sharding | Splitting a dataset horizontally across multiple database instances or nodes, usually by a key such as a user ID range or hash. |
| Partitioning | Splitting a table's data into segments — can be within one instance (for example, by date range) or across shards. |
| Replication | Copying data across multiple nodes for redundancy or read scaling. Can be synchronous (waits for replicas to confirm) or asynchronous. |
| Leader/Follower (Primary/Replica) | A replication topology where one node accepts writes and others replicate from it. |
| Consensus Algorithms (Raft, Paxos) | Protocols that let a cluster of nodes agree on a single value or state even with failures — underpinning leader election and distributed logs (for example, etcd uses Raft). |
| Consistent Hashing | A hashing technique used to distribute keys across nodes so that adding or removing a node only reshuffles a small fraction of keys. |
| Split Brain | A failure scenario where a cluster partitions and two nodes both believe they are the leader, risking data divergence. |

---

## 6. Schema, Indexing & Performance

| Term | Meaning |
| --- | --- |
| Index | An auxiliary data structure (often a B+Tree) that speeds up lookups at the cost of extra storage and slower writes. |
| Primary Key / Foreign Key | A primary key uniquely identifies a row; a foreign key references a primary key in another table to enforce referential integrity. |
| Normalization / Denormalization | Normalization organizes schema to reduce redundancy (1NF, 2NF, 3NF, and so on). Denormalization intentionally duplicates data to optimize read performance. |
| Materialized View | A query result that is physically stored and periodically refreshed, as opposed to a regular view that is computed on the fly. |
| Stored Procedure / Trigger | Precompiled logic stored in the database (procedure = called explicitly; trigger = fires automatically on an event such as `INSERT` or `UPDATE`). |
| Connection Pooling | Reusing a fixed set of open database connections across requests instead of opening and closing one per request, to reduce overhead. |

---

## 7. Workload Types & Data Movement

| Term | Meaning |
| --- | --- |
| OLTP | Online Transaction Processing — many small, fast read or write transactions (for example, an application's production database). |
| OLAP | Online Analytical Processing — complex queries over large volumes of data for analytics or reporting (for example, a data warehouse). |
| HTAP | Hybrid Transactional/Analytical Processing — systems designed to handle both workloads at once. |
| ETL / ELT | Extract, Transform, Load (transform before loading into the warehouse) vs. Extract, Load, Transform (load raw, transform inside the warehouse) — common data pipeline patterns. |
| CDC | Change Data Capture — streaming a record of row-level changes (inserts, updates, deletes) out of a database, often by tailing the WAL, for use in pipelines or replication. |

---

## Quick Reference: Acronym Cheat Sheet

- **DDL** — Data Definition Language
- **DML** — Data Manipulation Language
- **DQL** — Data Query Language
- **DCL** — Data Control Language
- **TCL** — Transaction Control Language
- **AST** — Abstract Syntax Tree
- **SST** — Sorted String Table
- **LSM** — Log-Structured Merge (tree)
- **WAL** — Write-Ahead Log
- **MVCC** — Multi-Version Concurrency Control
- **ACID** — Atomicity, Consistency, Isolation, Durability
- **BASE** — Basically Available, Soft state, Eventually consistent
- **CAP** — Consistency, Availability, Partition tolerance
- **2PC** — Two-Phase Commit
- **OLTP** — Online Transaction Processing
- **OLAP** — Online Analytical Processing
- **HTAP** — Hybrid Transactional/Analytical Processing
- **ETL/ELT** — Extract Transform Load / Extract Load Transform
- **CDC** — Change Data Capture
