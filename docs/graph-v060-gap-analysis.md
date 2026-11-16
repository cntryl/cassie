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
> exists. The gaps in the tables below are historical; the [qualification
> evidence](#qualification-evidence) section at the end lists what is proven and what is open.

Milestone: [v0.6.0 Fast, Idiomatic Graphs](https://github.com/cntryl/cassie/milestone/13).
Baseline: `main` at `e949a6689` (2026-11-13). The historical baseline predates the mapped-graph implementation
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
| D5 | Feature qualification needs [qualification](feature-qualification.md) evidence: oracle comparisons, resources, lifecycle and benchmarks. | P1 |

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
| 9 | [#1260](https://github.com/cntryl/cassie/issues/1260) Documentation/readiness: Qualify graph traversal with benchmarks, EXPLAIN evidence and a user guide | A12, B11, B12, D5 | 2-8 |

Out of milestone (gap kept here only): C2 `WITH RECURSIVE` acceleration and C4 graph algorithms.

## Qualification evidence

**Qualification review (2026-11-16): correctness checks passed; timing limits remain.** The
[executed closeout record](graph-v060-qualification.md) retains two native-driver
workflows, whole-path and clone/reference evidence, weighted routes at 100,000
mapped rows under the 64 MiB query budget, the nine topology points and their
shared artifact summary. All scenario correctness checks passed. Several local
five-sample release-profile timing rows failed quality; these observations do
not establish a trustworthy regression baseline or production capacity.
The second-driver and weighted-size correctness blockers below are resolved;
SQLx native composite discovery remains a named unsupported workflow. Future
performance qualification must evaluate the remaining timing/deployment evidence without
inventing an external-adoption requirement.

This section collects the evidence for that decision
([qualification criteria](feature-qualification.md)). Each entry below is a source
claim backed by a test or a document; none is a performance claim.

### Proven by tests and documents

| Area | Evidence |
| --- | --- |
| Mapped graph DDL: definition, key and endpoint validation, drop, rename, durable identities, discovery | [`property_graph_command_tests.rs`](../src/executor/execution/property_graph_command_tests.rs), [`property_graph_durability_tests.rs`](../src/executor/execution/property_graph_durability_tests.rs), [`property_graph_dependents_tests.rs`](../src/executor/execution/property_graph_dependents_tests.rs), [`virtual_views_property_graphs_tests.rs`](../src/catalog/virtual_views_property_graphs_tests.rs), [`persistence.rs`](../tests/graph_table/persistence.rs), [`database_scope.rs`](../tests/graph_table/database_scope.rs) |
| Lifecycle guards (`2BP01`, `0A000` for `CASCADE`), including renames and schema moves | [`property_graph_dependents_tests.rs`](../src/executor/execution/property_graph_dependents_tests.rs); guide section 10 run by [`graph_guide`](../tests/graph_guide/examples.rs) |
| `GRAPH_TABLE` semantics against an independent exhaustive oracle: directions, quantifiers, modes, selectors, tie order, key shapes, row and column storage | [`semantics.rs`](../tests/graph_table/semantics.rs), [`matrix.rs`](../tests/graph_table/matrix.rs), [`exact_reads.rs`](../tests/graph_table/exact_reads.rs), oracle in [`graph_oracle`](../tests/support/graph_oracle/mod.rs) |
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

### Qualification disposition and remaining limits

| Item | State |
| --- | --- |
| Read bounds | The documented read bounds and their counters are in [performance contracts](performance-contracts.md#graph-reads-v060-fast-reads-1253-1254-1259). `graph_weighted_paths` reads every selected edge row once for validation by contract (counted as `graph_validation_rows`, cost O(selected edges)) and its search reads only the neighbourhoods of the vertices it visits. No timing figure is recorded. |
| Real-driver qualification | Native path and array text/binary round trips, prepared parameters, live metadata, zero-hop/heterogeneous paths, exact keys and escaping passed through `tokio-postgres` and the public [Psycopg workflow](graph-psycopg-qualification.md). Psycopg uses explicit catalog metadata and TEXT casts; broader `CompositeInfo.fetch()` and SQLx native composite resolution are not claimed. SQLx builtin step/key/JSON/text results remain qualified. The prior second-client blocker is resolved; see the [actual receipt](evidence/graph-v060/psycopg.json). |
| Weighted routes at scale | All four 100k Tier 3 representatives passed correctness and resources with 64 MiB query memory, including three weighted routes over 100,000 mapped rows using indexed search and no fallback. Five measured samples were executed; weighted timing was noisy and the owner exited 101 on quality, not correctness. Exact observations and the separate 2 GiB native-storage budget are recorded in [qualification](graph-v060-qualification.md#benchmarks-and-measured-limits-1260). |
| Benchmark numbers | Exact local five-sample release-profile raw artifacts and nine one-sample topology smoke rows are committed under [evidence](evidence/graph-v060/manifest.json). Timing-quality failures remain failed; these separate owner runs do not form a complete single-head release suite or trustworthy baseline. Native deployment/capacity evidence remains separate. |
| Exact-head gates | The [qualification record](graph-v060-qualification.md#source-and-local-checks) separates the complete locked run from later affected suites and full strict Clippy/format/validators by source identity. Final-head hosted checks are recorded in the closing PR. |
| Guide scope | The guide documents the current dialect. Standard SQL/PGQ conformance is not claimed and no normative grammar comparison has been done. |

### Decisions recorded for batch A (2026-11-16, delegated Jev process)

- **Second real driver:** add `sqlx` as a dev-dependency of the integration test crate only
  (`postgres` and `runtime-tokio` features, no TLS backend, plaintext local connection).
  Judgment `sqlx_dev_dependency`: 0.96. Superseded in part at assembly: the round-trip tests
  failed at the resolver statement and were removed (judgment `sqlx_disposition`: 0.99), see the
  real-driver row above.
- **Weighted paths at scale:** replace load-validate-search with one streaming validation
  scan followed by a search over the lazy indexed store. Judgment
  `streaming_validation_then_indexed_search`: 1.00; its equivalence with the previous
  implementation was rated only 0.50, so the 360-case oracle matrix, the counted-read and the
  own-write tests are the proof obligation and run under the lazy indexed, exact plain and
  exact indexed paths.
- **Undeclared parameters at graph function arguments:** the parameter fold already holds the
  catalog and the database scope, so `$n` takes the reserved type from the function's
  declared argument type, as it does for `graph_weighted_paths`; the earlier blocker (the
  legacy inference had no database in scope) does not apply to the fold in
  `src/sql/parameters/fold_graph_functions.rs`.

## Executed client and topology probes

The explicit [Psycopg qualification](graph-psycopg-qualification.md) supplies a
second client workflow with ordinary registered composite and array adapters.
Its entrypoint is `qualification_graph_psycopg`; it is separate from the Cargo
correctness suite and requires its pinned Python environment. The five invariants passed through public Psycopg adapters; the
[qualification record](graph-v060-qualification.md) retains exact execution
evidence without expanding SQLx discovery.

`tier5_scaling_graph` adds three independent three-point rooted-tree curves:
out-degree 16/64/256 at one hop, depth 2/4/8 at fanout one, and fanout 2/4/8 at
three hops. Every fixture uses memory storage and the same 64 MiB query budget.
Results record actual mapped rows, fanout, depth, expected leaf cardinality,
setup duration, an indexed plain `EXPLAIN` preflight and runtime read counters.
The correctness oracle is the number of leaves of the generated rooted tree;
there are no performance timing assertions. These bounded curves complement,
and do not replace, the 100k-row Tier 3 representatives or five-sample release
measurements. The new analyzed fallback regression checks that a graph without
endpoint indexes reports full-row reads and retains its query results.

The repaired nine-point smoke and all-nine shared artifact summary passed on
`051af760`; public Psycopg passed all five invariants on `9b3aabae`. The
qualification record preserves earlier failures and the later green results.
The recorded timing-quality limits still constrain performance claims.
