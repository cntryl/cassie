# Graph v0.6.0 gap analysis

> Historical baseline and initial recommendations (2026-11-13). The selected
> [v0.6 contract and delivery plan](graph-v060-contract.md) and the live issue
> dependencies supersede this document's proposed grammar, selector rules,
> algorithm choices, migration assumptions, and backlog order. In particular,
> unbounded WALK is rejected even with a selector; existing graph data requires
> a preserving conversion; and node-only Dijkstra is not assumed correct for
> bounded paths. This document does not establish ISO conformance.
>
> The "Current implementation" section below describes the legacy `CREATE GRAPH`
> feature as it was at the baseline. That feature was removed in v0.6.0 without
> conversion (#1258): the statement, its backing tables, adjacency sidecars, the
> three table functions, their metrics and their benchmark no longer exist.
>
> Current state (2026-11-15): mapped property graphs, `GRAPH_TABLE`, the `cassie_graph` path
> types and `graph_weighted_paths` are implemented on exact reads, the legacy benchmark is
> replaced by mapped-graph benchmarks at tiers 1 to 3, and the [graph guide](graph-guide.md)
> exists. The gaps in the tables below are historical; the [promotion
> evidence](#promotion-evidence) section at the end lists what is proven and what is open.

Milestone: [v0.6.0 Fast, Idiomatic Graphs](https://github.com/cntryl/cassie/milestone/13).
Baseline: `main` at `e949a6689` (2026-11-13). Status today: graph traversal is **Experimental**
([feature support](feature-support.md)). This document lists the gaps between the current
implementation and a fast, ergonomic, idiomatic graph capability. It is the input for the
milestone backlog. It does not select contracts. Each decision gate below goes to Jev or the owner
before implementation.

## Current implementation (source-backed)

| Area | What exists | Source |
|---|---|---|
| DDL | `CREATE GRAPH g [(nodes (...), edges (...))]` creates the two backing tables `g_nodes` and `g_edges` with fixed reserved columns. Edge columns are `edge_id`, `source_type`, `source_id`, `target_type`, `target_id`, `edge_type` and `weight`. There is no `DROP GRAPH` or `ALTER GRAPH`. | `src/sql/parser/schema.rs:202`, `src/catalog/graphs.rs` |
| Query surface | Three positional table functions: `graph_neighbors` (6 args), `graph_expand` (7 args) and `graph_shortest_path` (9 args). `direction` is a `'out'`/`'in'`/`'both'` string. `edge_types` is a comma-separated string or `'*'`. All three return the same 13 columns, with `path_nodes` and `path_edges` as JSON. LATERAL works. | `src/sql/binder/table_functions.rs:92,144`, `src/executor/execution/graph.rs` |
| Storage | Every edge writes 4 derived adjacency keys: `OE`/`IE` ordered by edge type and `OW`/`IW` ordered by weight. A generation manifest is also written. Startup reconciles the keys. | `src/midge/adapter/key_encoding/graph.rs`, `src/midge/adapter/graphs.rs` |
| Native read | Before each neighborhood read, the code scans the **entire edge table and the entire adjacency family**. The read is then used only if all members verify. | `src/midge/adapter/graphs/verification.rs`, [graph qualification](graph-time-series-qualification.md) |
| Fallback | When the edge table has an uncommitted overlay, or verification fails, the code does a full row scan of the edge table **for every expanded node**. | `graph.rs:317-436` |
| Algorithms | `expand` is BFS over simple paths, not over nodes. `shortest_path` is uniform-cost search over a `Vec` frontier that is re-sorted on every pop, with no settled-node pruning. | `graph.rs:93-300` |
| Nodes table | Traversal never reads it. Node properties, node existence and node filters are not part of traversal. | `grep node_collection src/executor` returns nothing |
| Evidence | About 2.4k lines across 8 test modules plus a Tier 3 representative. The 100k representative needs 128 MiB and about 2.0M storage reads for **four** neighborhoods. | `tests/domain_models/graph/**`, `benches/tier3_system_query/graph.rs`, [performance contracts](performance-contracts.md) |

## Gaps

Severity: **P0** blocks "fast" or causes wrong results; **P1** is a core ergonomic or idiomatic gap;
**P2** is polish or a later capability.

### A. Performance

| # | Gap | Evidence | Sev |
|---|---|---|---|
| A1 | **Each hop costs O(E).** Verification scans every edge row and every adjacency key of the graph for each neighborhood. A depth-4 expansion over 100k edges costs about 2M reads and 76 MB. The "native" path is asymptotically slower than one sequential scan. | `verification.rs`; `performance-contracts.md:392` | P0 |
| A2 | **The overlay or fallback path is O(V·E).** Any pending write to the edge table in the session causes a full table scan for every frontier node. | `graph.rs:325-361` | P0 |
| A3 | **Completeness depends on a global verifier, not on an invariant.** Adjacency is a derived sidecar written in the same Midge transaction as the row, but reads do not trust it. A neighborhood-local proof is needed (for example a per-node degree and digest record, or verification only of the anchor's row-to-key mapping). Fixing this needs a persisted-format decision. Pre-release, so no migration is needed. | qualification doc "Open adjacency finding" | P0 (gate) |
| A4 | **The shortest-path frontier is re-sorted on every pop.** Each pop is O(F log F) with a scratch allocation. It should use a binary heap with a deterministic total-order key. | `graph.rs:233-237,296` | P1 |
| A5 | **There is no settled-node or dominance pruning.** k-shortest simple paths are found by enumerating paths, so the frontier grows exponentially on dense graphs even for `max_paths = 1`. Dijkstra should handle k = 1, and Yen or Eppstein-style handling should cover k > 1. | `graph.rs:496-537` | P1 |
| A6 | **`expand` enumerates paths, not nodes.** Each node can appear once per simple path, and the cycle check is O(depth) per edge. There is no node-visited mode, the usual reachability semantics. | `graph.rs:136-144` | P1 |
| A7 | **There is no bidirectional search** for point-to-point shortest path. | — | P2 |
| A8 | **There is no batching across LATERAL rows.** Each outer row re-runs verification and opens its own scans. Multi-source frontiers and shared verification are absent. | `execute_table_function` per call | P1 |
| A9 | **Every hot step allocates strings.** Keys are lowercased and node and edge IDs are copied at each hop, and edges are decoded from JSON payloads in the fallback. There is no interned or integer node ID. | `key_encoding/graph.rs`, `graph_edge_record_from_payload` | P2 |
| A10 | **The planner treats graph functions as opaque.** There is no cost or row estimate, and no pushdown of `WHERE edge_type = ...`, `depth <= n` or `LIMIT` into traversal arguments. | `planner/` | P2 |
| A11 | **Write amplification is 4 keys per edge plus a manifest update.** Whether both the weight-ordered and type-ordered families are needed should be re-justified once A3 lands. | `key_encoding/graph.rs` | P2 |
| A12 | **There is no graph benchmark beyond one Tier 3 representative.** Tier 1 key and frontier microbenchmarks, Tier 2 per-algorithm benchmarks, and degree, depth and fan-out scaling in Tier 5 are missing. | `benches/` | P1 |

### B. Ergonomics

| # | Gap | Evidence | Sev |
|---|---|---|---|
| B1 | **Arguments are positional only, with no defaults.** `graph_shortest_path` takes 9 positional arguments. There is no named notation (`=>`) or optional arguments such as `direction => 'out'` or `max_depth => 3`. | `table_functions.rs:92-94` | P1 |
| B2 | **Edge-type lists are comma-separated strings.** They should be `TEXT[]` (`ARRAY['knows','follows']`), and parameter binding should work. | `graph.rs:632` | P1 |
| B3 | **Paths come back as JSON.** `path_nodes` and `path_edges` should be native arrays or composite/record types, now that v0.3.0 added composites. | `table_functions.rs:144` | P1 |
| B4 | **All three functions return one wide 13-column shape.** For example, `neighbors` returns `path_rank` and `path_nodes`. Per-function result shapes are needed, with the edge row and the node row available to join. | same | P1 |
| B5 | **A graph cannot be declared over existing projection tables.** Graphs force `g_nodes`/`g_edges` with fixed column names, which conflicts with Cassie's read-model purpose because projectors already own their tables. A form is needed that maps a graph onto existing tables and columns. | `catalog/graphs.rs`, `GraphMeta::new` | P0 |
| B6 | **Node identity is TEXT-only, as a (type, id) pair.** Numeric IDs are converted with `to_string` (`1` vs `1.0` drift), and there is no UUID or BIGINT key. Untyped or single-label graphs still have to supply a type string. | `graphs.rs:381` | P1 |
| B7 | **Traversal cannot filter on node or edge properties.** Only edge type is supported. Typical needs include "follow edges where `active`" and "stop at nodes of type X". | `graph_edge_matches` | P1 |
| B8 | **The nodes table is never used.** Edges can point at nonexistent nodes, and results cannot return node properties without a manual join. | executor | P1 |
| B9 | **Weight is optional and defaults silently to 1.0.** A non-numeric weight returns a generic error. There is no unweighted mode, no choice of weight column, and no `max_cost` bound. | `graphs.rs:390` | P1 |
| B10 | **There is no `DROP GRAPH`, `ALTER GRAPH ... RENAME`, `\d`-style introspection, `pg_catalog`/`information_schema` view of graphs, or REST/Admin UI listing.** | `grep "drop graph"` returns nothing | P1 |
| B11 | **`EXPLAIN` does not show the native or fallback choice, reads, frontier or verification cost** as a first-class plan node. Evidence goes only to runtime metrics. | `GraphExecutionEvidence::publish` | P2 |
| B12 | **There is no user-facing guide.** Graph usage appears only in tests and in one line of `feature-support.md`. | `docs/` | P1 |

### C. Idiomatic SQL

| # | Gap | Notes | Sev |
|---|---|---|---|
| C1 | **There is no standard graph query syntax.** SQL:2023 Part 16 (SQL/PGQ) defines `CREATE PROPERTY GRAPH ... VERTEX TABLES / EDGE TABLES` and `GRAPH_TABLE (g MATCH (a)-[e:knows]->{1,3}(b) WHERE ... COLUMNS (...))`. PostgreSQL is developing this, and Oracle 23ai and DuckDB (duckpgq) ship it. It is the idiomatic target, and it also solves B5 because it declares a graph over existing tables. | Decision gate: full or partial PGQ, or functions only | P0 (gate) |
| C2 | **Recursive CTEs are not accelerated.** `WITH RECURSIVE ... SEARCH ... CYCLE` already parses and executes. Recognizing traversal-shaped recursive CTEs over a graph's edge table and routing them to adjacency reads would make plain PostgreSQL code fast. | `src/sql/parser/query_cte.rs`, `binder/recursive.rs` | P1 |
| C3 | **Function names and conventions do not follow common idioms.** pgRouting-style `(seq, path_seq, node, edge, cost, agg_cost)` and Apache AGE / pgRouting naming could serve as compatibility references if functions are kept. | Decide alongside C1 | P2 |
| C4 | **There are no graph algorithms beyond traversal.** Connected components, degree, PageRank and similar algorithms are commonly needed in read models. Scope belongs on the [read-model scope](product-roadmap.md) line. | Probably post-v0.6.0 | P2 |

### D. Correctness and semantics to settle

| # | Gap | Sev |
|---|---|---|
| D1 | `sort_shortest_frontier` ties break on `(cost, node_id)` only. The order of equal-cost paths to the same node is not a total order over full paths, so `path_rank` stability is unproven. | P1 |
| D2 | Node type compares case-insensitively and node ID compares case-sensitively, and edge type is folded. These rules are implicit and should be one documented identity contract, which interacts with B6. | P1 |
| D3 | Rows with a missing endpoint or type are silently skipped as non-edges, and negative weights error only on the strict path. Contracts are needed for invalid edges at write time versus read time. | P1 |
| D4 | The `expand` stop reason reports `"limit"` whenever any edge was expanded, even when traversal ended because it was exhausted. | P2 |
| D5 | Promotion from Experimental to Stable needs [experimental-promotion-criteria](experimental-promotion-criteria.md) evidence: oracle comparisons, resources, lifecycle and benchmarks. | P1 |

## Decision gates (resolve before dependent implementation)

1. **Query surface (C1, B1-B4, C2).** Options: (a) SQL/PGQ `GRAPH_TABLE` as the primary surface, with the existing functions kept as sugar; (b) redesigned named-argument functions only; (c) recursive-CTE acceleration only. Recommendation: (a) a bounded PGQ subset (vertex and edge table mapping, single-pattern `MATCH` with quantified paths `{m,n}`, label and property `WHERE`, `COLUMNS`), plus C2 so plain recursive CTEs are also fast.
2. **Graph definition (B5, B6, B8).** Graphs over existing tables and columns with declared key columns of any supported type, versus the current owned `_nodes`/`_edges` tables. Recommendation: mapping onto existing tables. Pre-release, so the old form can be removed rather than kept.
3. **Neighborhood completeness proof (A1-A3).** This selects the persisted adjacency format that permits O(degree) verified reads. The Midge published-API boundary applies, and the decision needs Jev review plus a missing-member regression like DOM004.
4. **Algorithm semantics (A4-A6, D1).** Node versus path semantics for expand, k-shortest algorithm, and total-order tie-break.

## Gate 1 and 2 decisions (Jev, 2026-11-13)

Jev selected these directions after reviewing the read-model scope, the current implementation, the candidate surfaces and the prior art (DuckPGQ, Neptune, T-SQL graph). The request and response files are in the ignored `target/graph-v060-gates/` directory. The contract issue records the final wording.

| Question | Result | Action |
|---|---|---|
| Primary query surface | `pgq_subset_plus_functions` 0.73 (named functions only 0.20) | Adopt a bounded SQL/PGQ `GRAPH_TABLE … MATCH` subset, plus named-argument functions for weighted and k-shortest paths |
| Graph definition | `CREATE PROPERTY GRAPH` over existing tables, replacing `CREATE GRAPH`, 0.98 | Remove the owned `g_nodes`/`g_edges` form |
| Path mode | Standard `WALK` default with `TRAIL`/`ACYCLIC`/`SIMPLE` modes, 0.96 | Follow the standard. Current simple-path behavior becomes explicit `SIMPLE` |
| Remove `graph_neighbors`/`graph_expand` | 0.75 | Remove them |
| Defer `WITH RECURSIVE` acceleration | 0.59 | Defer out of v0.6.0. C2 stays in this document only, with no issue |
| Require quantifier upper bound | 0.86 | Adopt, as amended by the probe below |
| Subset misses a typical read (adversarial) | **0.74**, at or above 0.30, so it was probed | Probe: the gap is unbounded hierarchy closure (0.94). Fix: allow `*`/`+` under `TRAIL`, `ACYCLIC` or `SIMPLE` path modes, or under a selector, still governed by the 54000 memory budget (0.93). An unbounded `WALK` is rejected |

Gates 3 (completeness proof) and 4 (algorithm semantics) are decided inside the issues that own them.

## Milestone backlog

| Order | Issue | Covers | Depends on |
|---|---|---|---|
| 1 | [#1252](https://github.com/cntryl/cassie/issues/1252) Feature request: Define the v0.6.0 property graph and GRAPH_TABLE contract | Gates 1, 2 and 4, C1, D2, D3 | — |
| 2 | [#1253](https://github.com/cntryl/cassie/issues/1253) Enhancement: Serve native graph neighborhoods in O(degree) reads with local completeness proof | A1, A3, A11 | — (gate 3 is decided here) |
| 3 | [#1254](https://github.com/cntryl/cassie/issues/1254) Enhancement: Keep native graph reads within transactions with pending edge writes | A2 | 2 |
| 4 | [#1255](https://github.com/cntryl/cassie/issues/1255) Feature request: Define property graphs over existing projection tables | B5, B6, B8, B10, D3 | 1 |
| 5 | [#1256](https://github.com/cntryl/cassie/issues/1256) Feature request: Parse and bind the bounded GRAPH_TABLE MATCH subset | C1, B2, B4, B7 | 1, 4 |
| 6 | [#1257](https://github.com/cntryl/cassie/issues/1257) Enhancement: Execute graph patterns with node-visited traversal, heap Dijkstra and deterministic k-shortest paths | A4-A7, D1, D4, B3 | 2, 5 |
| 7 | [#1258](https://github.com/cntryl/cassie/issues/1258) Enhancement: Replace legacy graph table functions with a named-argument weighted path function | B1, B9, C3 | 1, 6 |
| 8 | [#1259](https://github.com/cntryl/cassie/issues/1259) Enhancement: Batch graph traversal across LATERAL rows and push predicates into traversal | A8, A9, A10 | 6 |
| 9 | [#1260](https://github.com/cntryl/cassie/issues/1260) Documentation/readiness: Qualify graph traversal for Stable with benchmarks, EXPLAIN evidence and a user guide | A12, B11, B12, D5 | 2-8 |

Out of milestone (gap kept here only): C2 `WITH RECURSIVE` acceleration and C4 graph algorithms.

## Promotion evidence

This section collects the evidence for the decision whether graph support moves out of
Experimental ([promotion criteria](experimental-promotion-criteria.md)). It does not change
the support status, which is recorded in [Feature support](feature-support.md) and stays
Experimental until the integrator decides from this list. Each entry below is a source
claim backed by a test or a document; none is a performance claim.

### Proven by tests and documents

| Area | Evidence |
| --- | --- |
| Mapped graph DDL: definition, key and endpoint validation, drop, rename, durable identities, discovery | [`property_graph_command_tests.rs`](../src/executor/execution/property_graph_command_tests.rs), [`property_graph_durability_tests.rs`](../src/executor/execution/property_graph_durability_tests.rs), [`property_graph_dependents_tests.rs`](../src/executor/execution/property_graph_dependents_tests.rs), [`virtual_views_property_graphs_tests.rs`](../src/catalog/virtual_views_property_graphs_tests.rs), [`persistence.rs`](../tests/graph_table/persistence.rs), [`database_scope.rs`](../tests/graph_table/database_scope.rs) |
| Lifecycle guards (`2BP01`, `0A000` for `CASCADE`), including renames and schema moves | [`property_graph_dependents_tests.rs`](../src/executor/execution/property_graph_dependents_tests.rs); guide section 10 run by [`graph_guide`](../tests/graph_guide/examples.rs) |
| `GRAPH_TABLE` semantics against an independent exhaustive oracle: directions, quantifiers, modes, selectors, tie order, key shapes, row and column storage | [`semantics.rs`](../tests/graph_table/semantics.rs), [`matrix.rs`](../tests/graph_table/matrix.rs), [`exact_reads.rs`](../tests/graph_table/exact_reads.rs), oracle in [`graph_oracle`](../tests/support/graph_oracle) |
| Own writes, savepoints, other-session isolation and snapshot reads | [`transactions.rs`](../tests/graph_table/transactions.rs) |
| Resource controls: memory budget `54000` and cancellation `57014` | [`resources.rs`](../tests/graph_table/resources.rs) |
| Path types, constructors, validation, JSON export, catalog discovery, restart | [`graph_types`](../tests/graph_types.rs) |
| Wire behavior: Describe metadata, text and binary results and parameters, no row emitted for a failing evaluation | [`wire.rs`](../tests/graph_table/wire.rs), [`wire.rs`](../tests/graph_types/wire.rs); these use the repository's protocol harness |
| Weighted paths against an exhaustive oracle, weight validation, arguments, permissions, resources | [`graph_weighted`](../tests/graph_weighted.rs) |
| Read authority: a graph grants nothing | [`security.rs`](../tests/graph_weighted/security.rs); guide section 8 |
| Admin catalog `property_graphs` section | [`property_graphs.rs`](../tests/rest/queries/admin_query/property_graphs.rs), [`openapi.yml`](../public/openapi.yml) |
| User guide whose every SQL example is executed and compared | [`graph_guide`](../tests/graph_guide.rs), [graph guide](graph-guide.md) |
| Benchmarks exist at tiers 1 to 3, are registered with the `graph` workload family, compile and run to completion once in smoke mode with results checked against Rust oracles | [performance contracts](performance-contracts.md#graph-benchmarks) |
| Legacy feature removed without conversion | [contract](graph-v060-contract.md), `f54952d08` |

### Still open

| Item | State |
| --- | --- |
| Read bounds | Every graph read is an exact read of the mapped tables. No O(degree) bound is claimed or measured, and `graph_weighted_paths` reads every edge by design. Fast reads (#1253, #1254 read-bound part, #1259) are not part of the current build. |
| `EXPLAIN` and `EXPLAIN ANALYZE` evidence | The graph plan node is pending from its owning lane. Until it reports an access path, the tier 3 owner declares `graph_table_exact_read` in its metadata. |
| Real-driver qualification | The wire evidence above uses the repository's protocol harness. A qualification with at least one real PostgreSQL driver for text and binary composite results is not recorded; the contract asks for two. |
| Tier 3 scale | The Tier 3 scenarios keep the 100,000-row contract class, but exact reads under the 64 MiB profile failed with `54000` from about 30,000 mapped rows, so the fixtures map 15,000 to 28,000 rows. See [performance contracts](performance-contracts.md#graph-benchmarks). |
| Benchmark numbers | No timing or scaling number is recorded. Gate-quality results need the release profile with at least five samples, host identity and a baseline. |
| Exact-head gates | The ordered local gate and the hosted checks run on the integrated head are owned by the assembler and are not recorded here. |
| Guide scope | The guide documents the current dialect. Standard SQL/PGQ conformance is not claimed and no normative grammar comparison has been done. |
