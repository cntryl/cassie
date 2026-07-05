# Architecture Diagrams

This reference documents Cassie's current architecture with Mermaid-in-Markdown diagrams. The diagrams are source-only Markdown so GitHub can render them; no generated PNG or SVG artifacts are checked in.

Use this with [Module Organization](module-organization.md), [Feature Ownership](feature-ownership.md), [Performance Contracts](performance-contracts.md), and [Production Readiness](production-readiness.md). Code links point to the current source of truth for the shape being diagrammed.

## Top-Level Module Ownership

```mermaid
flowchart TD
    subgraph client_surfaces["Client and API surfaces"]
        pgwire["src/pgwire\nPostgreSQL wire protocol"]
        rest["src/rest\nHTTP REST and admin API"]
        embedded["cassie::Cassie\nEmbedded Rust API"]
    end

    subgraph app_layer["Application services"]
        app["src/app\nCassie facade, lifecycle, auth, query, replay, snapshots"]
        runtime["src/runtime*\nmetrics, controls, caches, epochs, feedback"]
        catalog["src/catalog\nschema, metadata, virtual views, roles"]
    end

    subgraph query_layer["SQL query engine"]
        sql["src/sql\nparser, AST, binder, functions"]
        planner["src/planner\nlogical, optimizer, physical plans"]
        executor["src/executor\noperators, commands, access paths"]
    end

    subgraph accelerators["Search, vector, AI, and analytics"]
        search["src/search\nBM25, analyzer, inverted index"]
        vector["src/vector\ndistance metrics, HNSW, IVFFlat"]
        hybrid["src/hybrid\ntext plus vector scoring"]
        embeddings["src/embeddings\nprovider contracts and validation"]
    end

    subgraph storage_layer["Storage"]
        midge["src/midge\nConcrete cntryl-midge adapter"]
        types["src/types\nvalues, schema, row and vector types"]
    end

    pgwire --> app
    rest --> app
    embedded --> app
    app --> sql
    sql --> planner
    planner --> executor
    executor --> midge
    executor <--> catalog
    app <--> catalog
    app <--> runtime
    executor --> runtime
    pgwire --> runtime
    rest --> runtime
    executor --> search
    executor --> vector
    executor --> hybrid
    app --> embeddings
    embeddings --> vector
    search --> midge
    vector --> midge
    catalog --> midge
    types --> sql
    types --> planner
    types --> executor
    types --> midge
```

Primary evidence:
[src/lib.rs](../src/lib.rs),
[src/app/mod.rs](../src/app/mod.rs),
[src/executor/mod.rs](../src/executor/mod.rs),
[src/midge/adapter/mod.rs](../src/midge/adapter/mod.rs),
[Feature Ownership](feature-ownership.md), and [Module Organization](module-organization.md).

## Client Entrypoints

```mermaid
flowchart LR
    simple_client["Pgwire simple query client"] --> simple_conn["connection.rs\nhandle_simple_query"]
    extended_client["Pgwire extended query client"] --> extended_conn["connection/extended.rs\nparse, bind, describe, execute"]
    rest_client["HTTP client"] --> rest_router["rest/router.rs\nroute_request and route_dispatch"]
    embedded_client["Embedded Rust caller"] --> cassie_api["Cassie methods"]

    simple_conn --> pg_blocking["run_pgwire_blocking\npgwire_simple_query"]
    extended_conn --> pg_blocking
    rest_router --> rest_blocking["run_rest_blocking\nrest_route, rest_auth, rest_embedding_search"]

    pg_blocking --> session["CassieSession and auth"]
    rest_blocking --> session
    cassie_api --> session

    session --> execute_sql["Cassie::execute_sql\nor execute_parsed_sql_with_mode"]
    session --> document_api["Documents, indexes, search,\nconsistency, snapshots, operational APIs"]
    execute_sql --> query_pipeline["Parse, bind, plan, execute"]
    document_api --> storage_services["App services and Midge adapter"]
```

Primary evidence:
[src/pgwire/connection.rs](../src/pgwire/connection.rs),
[src/pgwire/connection/extended.rs](../src/pgwire/connection/extended.rs),
[src/pgwire/connection/blocking.rs](../src/pgwire/connection/blocking.rs),
[src/rest/router.rs](../src/rest/router.rs), and
[src/app/query.rs](../src/app/query.rs).

## Query Execution Sequence

```mermaid
sequenceDiagram
    participant Client
    participant Transport as Transport surface
    participant App as Cassie app layer
    participant SQL as SQL parser and binder
    participant Planner as Planner and query caches
    participant Executor as Executor
    participant Store as Midge adapter
    participant Runtime as RuntimeState

    Client->>Transport: query, route, or API call
    Transport->>App: authenticated session call
    App->>Runtime: start controls, timers, and metrics
    App->>SQL: parse and bind statement
    SQL-->>App: parsed or bound statement
    App->>Planner: lookup cached plan or compile physical plan
    Planner->>Runtime: cache hits, misses, adaptive metadata
    Planner-->>App: physical plan and provenance
    App->>Executor: run_with_session_controls
    Executor->>Store: document, index, metadata, and sidecar reads or writes
    Store-->>Executor: rows, write reports, or storage errors
    Executor->>Runtime: access-path, storage, scoring, and operator metrics
    Executor-->>App: QueryResult or QueryError
    App->>Runtime: result cache, feedback, data epoch, error metrics
    App-->>Transport: result or CassieError
    Transport-->>Client: wire frames, JSON response, or Rust result
```

Primary evidence:
[src/app/query.rs](../src/app/query.rs),
[src/sql/binder.rs](../src/sql/binder.rs),
[src/planner/physical.rs](../src/planner/physical.rs),
[src/executor/execution/entrypoints.rs](../src/executor/execution/entrypoints.rs), and
[src/runtime.rs](../src/runtime.rs).

## Read Path And Accelerator Selection

```mermaid
flowchart TD
    physical["PhysicalPlan\nread metadata, operators, adaptive candidates"] --> preferred{"Preferred scalar route?"}
    preferred -->|yes| scalar_first["Scalar index path\nindex_seek, prefix_scan,\nrange_scan, ordered_bounded_scan"]
    preferred -->|no| registry["Access path registry"]
    scalar_first --> scalar_hit{"Rows produced?"}
    scalar_hit -->|yes| result["Batch rows"]
    scalar_hit -->|no| registry

    registry --> vector_path["Vector distance path\nHNSW, IVFFlat metadata,\nnormalized-vector sidecars"]
    vector_path --> vector_hit{"Rows produced?"}
    vector_hit -->|yes| result
    vector_hit -->|no| scored_path["Scored path\nfull-text, vector top-k,\nhybrid scoring"]

    scored_path --> scored_hit{"Rows produced?"}
    scored_hit -->|yes| result
    scored_hit -->|no| analytical_path["Analytical projection path\ncolumn-store and materialized projection options"]

    analytical_path --> analytical_hit{"Rows produced?"}
    analytical_hit -->|yes| result
    analytical_hit -->|no| scalar_registry["Scalar index path"]

    scalar_registry --> scalar_registry_hit{"Rows produced?"}
    scalar_registry_hit -->|yes| result
    scalar_registry_hit -->|no| column_path["Ordered column path\ncolumn batches and covered scans"]

    column_path --> column_hit{"Rows produced?"}
    column_hit -->|yes| result
    column_hit -->|no| projected_path["Projected filtered path\nmaterialized projection reads"]

    projected_path --> projected_hit{"Rows produced?"}
    projected_hit -->|yes| result
    projected_hit -->|no| rollup_path["Rollup path\nprecomputed aggregates and time buckets"]

    rollup_path --> rollup_hit{"Rows produced?"}
    rollup_hit -->|yes| result
    rollup_hit -->|no| fallback["Source query fallback\njoins, CTEs, sets, scans,\nfilters, sorts, aggregates"]

    fallback --> result
    result --> metrics["EXPLAIN and RuntimeState metrics\naccess_path, fallback_reason,\ntop_k_mode, join_strategy"]
```

The registry order is implemented in [src/executor/execution/dispatch.rs](../src/executor/execution/dispatch.rs). The required access-path vocabulary and diagnostics are documented in [Performance Contracts](performance-contracts.md). Storage-side accelerator modules live under [src/midge/adapter](../src/midge/adapter), with search and vector logic in [src/search](../src/search), [src/vector](../src/vector), and [src/hybrid](../src/hybrid).

## Write And Admin Flows

```mermaid
flowchart TD
    dml["SQL DML\nINSERT, UPDATE, DELETE,\ntransactions"] --> dml_exec["executor/execution/dml_command.rs\nand dml.rs"]
    ddl["SQL DDL and metadata commands"] --> schema_exec["executor/execution/schema_command.rs\nplus graph, vector, sequence commands"]
    rest_docs["REST document and index routes"] --> rest_services["rest documents, indexes,\nsearch, consistency"]
    replay["Projection replay and refresh"] --> app_replay["app replay and write_refresh"]
    repair["Projection verification,\ndiffing, repair"] --> consistency["app consistency and\nexecutor projection repair"]
    snapshots["Snapshots and restore"] --> app_snapshots["app snapshots"]
    ops["Operational assignments"] --> app_operational["app operational records"]

    dml_exec --> midge_writes["Midge document writes\nrows, scalar indexes,\nvector sidecars, metadata"]
    schema_exec --> catalog_update["Catalog and Midge metadata update"]
    rest_services --> midge_writes
    app_replay --> midge_writes
    consistency --> reports["Midge verification,\ncomparison, repair reports"]
    app_snapshots --> snapshot_files["Midge directory snapshot\nand manifest"]
    app_operational --> assignments["Midge assignment metadata"]

    midge_writes --> data_epoch["Runtime data epoch\nand write metrics"]
    catalog_update --> schema_epoch["Midge schema epoch\nRuntime schema epoch"]
    schema_epoch --> cache_invalidate["Plan/result cache invalidation\nruntime feedback reset"]
    data_epoch --> derived["Rollup refresh and\nmaterialized projection staleness"]
    reports --> catalog_views["Catalog virtual views\nand REST admin reports"]
    assignments --> catalog_views
```

Primary evidence:
[src/executor/execution/dml_command.rs](../src/executor/execution/dml_command.rs),
[src/executor/execution/dml.rs](../src/executor/execution/dml.rs),
[src/executor/execution/schema_command.rs](../src/executor/execution/schema_command.rs),
[src/app/registry.rs](../src/app/registry.rs),
[src/app/replay.rs](../src/app/replay.rs),
[src/app/consistency.rs](../src/app/consistency.rs),
[src/app/snapshots.rs](../src/app/snapshots.rs),
[src/app/operational.rs](../src/app/operational.rs), and
[Projection Repair Runbook](projection-repair-runbook.md).

## Cross-Cutting State

```mermaid
flowchart LR
    cassie["Cassie\nArc<Midge>, Catalog,\nEmbeddingProvider, RuntimeState"] --> runtime["RuntimeState"]
    cassie --> catalog["In-memory Catalog"]
    cassie --> midge["Midge adapter"]
    cassie --> app_caches["App-local caches\nnormalized vectors,\nquery embeddings,\nvector search results"]

    runtime --> metrics["Metrics snapshots\nruntime, query, pgwire, rest,\nstorage, search, vector,\nhybrid, projections, read paths"]
    runtime --> plan_cache["L1 plan cache"]
    runtime --> result_cache["Execution result cache"]
    runtime --> feedback["Runtime feedback\noperator estimates"]
    runtime --> epochs["Schema epoch,\ndata epoch,\nindex feedback epoch"]
    runtime --> controls["Query controls\nlimits, timeout,\nrunning query guards"]

    midge --> durable_cache["Durable plan and feedback records"]
    midge --> persisted_catalog["Persisted schema, roles,\nindexes, projections,\nrollups, assignments"]
    midge --> sidecars["Sidecar data\nscalar indexes, full-text,\ncolumn batches, normalized vectors"]

    persisted_catalog --> hydration["startup hydration"]
    durable_cache --> hydration
    hydration --> catalog
    hydration --> runtime
    sidecars --> app_caches
    epochs --> plan_cache
    epochs --> result_cache
```

Primary evidence:
[src/app/state.rs](../src/app/state.rs),
[src/app/lifecycle.rs](../src/app/lifecycle.rs),
[src/app/hydration.rs](../src/app/hydration.rs),
[src/runtime.rs](../src/runtime.rs),
[src/runtime/query_cache.rs](../src/runtime/query_cache.rs), and
[src/midge/adapter/metadata.rs](../src/midge/adapter/metadata.rs).

## Architecture Drift Analysis

These findings are intentionally tied to diagrams and code evidence. They do not change public Rust APIs, SQL behavior, protocol behavior, or storage format.

| Category | Finding | Evidence | Impact | Owner area and follow-up |
| --- | --- | --- | --- | --- |
| Confirmed drift | Several source and test files still exceed the 1,000-line module target. Use the repository audit command in `AGENTS.md` for the current list before planning broad work. | [Module Organization targets](module-organization.md), current source audit | Large files are the clearest current drift from the small-module architecture and make unrelated query, storage, and runtime changes harder to review. | Owners: runtime, midge adapter, executor, catalog, app, and tests. Follow-up: before adding substantial behavior in an oversized file, extract a focused module/test file or record a current split plan. |
| Confirmed drift | `src/app/query.rs` is both a query pipeline coordinator and an owner of plan-cache provenance, result-cache keys, feedback capture, EXPLAIN ANALYZE deltas, timeout checks, and data-epoch mutation. | [Query Execution Sequence](#query-execution-sequence), [src/app/query.rs](../src/app/query.rs), [src/app/cache.rs](../src/app/cache.rs), [src/runtime.rs](../src/runtime.rs) | Query work can accidentally change caching, feedback, epoch, or diagnostics behavior in the same file. | Owner: app/query and runtime. Follow-up: the next broad query-pipeline change should split plan/cache resolution and feedback capture into focused helpers before adding behavior. |
| Candidate risk | Access-path registry order is architecture-significant. New read accelerators can change results or performance if they enter the registry without matching EXPLAIN and metric assertions. | [Read Path And Accelerator Selection](#read-path-and-accelerator-selection), [src/executor/execution/dispatch.rs](../src/executor/execution/dispatch.rs), [Performance Contracts](performance-contracts.md) | Correct row results can hide a degraded Midge access path, especially when fallback scans still pass functional tests. | Owner: planner, executor, runtime metrics. Follow-up: every new access path needs plan assertions, EXPLAIN labels, runtime metrics, and fallback-reason tests in the same slice. |
| Candidate risk | Startup hydration is not purely read-only. It can rebuild normalized vectors for persisted vector indexes and rebuild cardinality stats when stored stats are missing or stale. | [Cross-Cutting State](#cross-cutting-state), [src/app/hydration.rs](../src/app/hydration.rs), [src/midge/adapter/metadata.rs](../src/midge/adapter/metadata.rs), [Production Readiness](production-readiness.md) | Larger vector or high-cardinality deployments may see startup latency and write amplification before the operator has a production-ready guidance surface. | Owner: app hydration, midge metadata, vector. Follow-up: before promoting larger-corpus vector/search readiness, define persisted validity markers, lazy rebuild, or bounded background rebuild behavior. |
| Candidate risk | REST remains secondary/admin, but its route table touches documents, indexes, vector search, consistency reports, and auth. It is compact today, but new admin routes can blur public query and operator workflows. | [Client Entrypoints](#client-entrypoints), [src/rest/router.rs](../src/rest/router.rs), [Feature Ownership](feature-ownership.md), [Production Readiness](production-readiness.md) | REST could grow into a parallel primary interface with weaker compatibility and diagnostics guarantees than pgwire. | Owner: REST and app services. Follow-up: keep REST additions administrative or explicitly document any user-visible overlap with pgwire in [PostgreSQL Compatibility](postgres-compatibility.md) and [Feature Support](feature-support.md). |
| Accepted tradeoff | Pgwire and REST are async transports over a synchronous engine. Blocking work is intentionally isolated behind `spawn_blocking` boundaries. | [Client Entrypoints](#client-entrypoints), [src/pgwire/connection/blocking.rs](../src/pgwire/connection/blocking.rs), [src/rest/router.rs](../src/rest/router.rs), [Runtime-Boundary Contract](performance-contracts.md#runtime-boundary-contract-phase-04) | Engine internals stay simpler, but every transport change must preserve the boundary to avoid blocking Tokio worker tasks. | Owner: pgwire, REST, runtime. Follow-up: new protocol or route work must use the existing blocking helpers and keep transport-boundary tests current. |
| Accepted tradeoff | Midge is the only storage layer. Indexes, projections, search, vector sidecars, snapshots, and metadata use the concrete adapter instead of a backend trait. | [Top-Level Module Ownership](#top-level-module-ownership), [src/midge/adapter/mod.rs](../src/midge/adapter/mod.rs), [src/midge/adapter/key_encoding.rs](../src/midge/adapter/key_encoding.rs), [Module Organization](module-organization.md#midge-adapter) | Storage format and accelerator behavior are tightly coupled to Midge locality, which is intentional for V1 but raises migration cost for persisted key changes. | Owner: midge adapter and catalog. Follow-up: all persisted key work must continue through `key_encoding.rs` and include an explicit migration plan for storage-layout changes. |
| Accepted tradeoff | The diagrams include stable and experimental surfaces. The architecture map is broader than production-ready support, and no feature family is production-ready by default. | [Production Readiness](production-readiness.md), [Product Roadmap](product-roadmap.md), [Experimental Promotion Criteria](experimental-promotion-criteria.md) | Readers must not infer production guarantees from a surface appearing in the architecture diagrams. | Owner: docs and feature owners. Follow-up: when a surface promotes, update readiness, support, compatibility, and this architecture map in the same slice. |

## Maintenance Rules

- Update these diagrams when a module boundary, client entrypoint, storage sidecar, runtime cache, or admin workflow changes.
- Keep Mermaid node IDs stable and labels short enough for GitHub rendering.
- Do not add generated diagram artifacts unless the repository adopts a deterministic Mermaid render step.
- Drift findings should stay evidence-linked and should name a concrete owner and follow-up.
