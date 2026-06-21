# Phase 04: Foundation Contracts

Phase 04 makes Cassie's runtime and access-path contracts explicit before write or read optimization changes implementation.

The goal is not to optimize the engine yet.
The goal is to define the boundaries later phases must preserve: async at transport, shutdown, and task-coordination edges; synchronous planner, executor, catalog, storage, auth, and embedding contracts; and read-model access-path vocabulary that write and read work can share.

Pgwire and REST are async interfaces.
They must not run blocking engine, authentication, storage, or embedding-provider work directly on Tokio IO tasks.
SQL and pgwire are read interfaces.
They must not imply that a generic SQL execution path is sufficient for read-model contracts when Cassie needs a Midge-native access path.

## Core Rules

A runtime path is not correct merely because it returns the right response.
A runtime path is correct only when async code performs async IO and delegates synchronous engine work through an explicit blocking boundary.

A read access pattern is not a contract merely because it returns correct rows.
A read access pattern is a contract only when it names the intended Midge-efficient access path, the forbidden fallback shape, and the storage/index/key grouping expectation later phases must preserve.

Each phase 04 issue must define the relevant contract surface:

- async entrypoints and synchronous engine ownership
- required blocking-boundary behavior
- forbidden direct blocking behavior on Tokio worker tasks
- error, cancellation, timeout, and shutdown behavior
- read access-path vocabulary when later write/read work depends on it
- tests, diagnostics, or static audits that keep the contract visible

Each issue should include a concrete `Implementation Plan` section with expected modules, TDD order, diagnostics, validation, and close-out sequence.
The goal is that implementation work is mostly mechanical once the issue is picked up.

## Boundary Categories

| Boundary | Purpose | Expected path |
| --- | --- | --- |
| Pgwire socket IO | Read and write PostgreSQL wire frames | async socket IO, sync query/auth work offloaded explicitly |
| REST HTTP IO | Accept requests and collect bodies | async connection/body IO, sync route handlers offloaded explicitly |
| Authentication | Verify roles and passwords | synchronous catalog/storage/hash work behind a blocking boundary when called from async transport |
| Query execution | Parse, plan, execute, and encode results | synchronous engine API, never hidden as async internals |
| Document and index APIs | REST collection/document/index/search operations | synchronous app/rest handlers behind a blocking boundary |
| Embeddings | Blocking provider HTTP and retry sleeps | synchronous provider trait, isolated from Tokio IO workers |
| Shutdown and cancellation | Stop listeners and finish in-flight work predictably | async coordination with bounded blocking-task semantics |
| Diagnostics | Prove boundary behavior | runtime counters, logs, or static audit coverage |

## Phase Sequence

1. Runtime boundary contracts: define the supported async/sync split and forbidden paths.
2. Auth and embedding blocking discipline: classify blocking auth/provider work before transport helpers are written.
3. Pgwire blocking boundary: move query, describe, execute, and auth work behind explicit blocking calls.
4. REST blocking boundary: move sync route handlers behind explicit blocking calls while keeping body IO async.
5. Runtime boundary diagnostics: make boundary usage and fallback visible.
6. Boundary regression tests and static audit: prevent future async/sync drift.
7. Read access-path contracts: define the read-shape vocabulary and storage/index/key grouping expectations consumed by phase 05 write work and phase 06 read work.

## Non-Goals

- No second storage abstraction.
- No async trait migration for planner, executor, storage, auth, or embedding providers by default.
- No Dotnet-style async cascade through synchronous Rust engine code.
- No change to SQL, pgwire, REST, auth, embedding, or timeout semantics unless a focused issue explicitly narrows a bug.
- No hidden unbounded task spawning that makes backpressure, cancellation, or shutdown less predictable.
- No write-path index/key-layout optimization before phase 04 issue 07 names the read shape it must preserve.
- No phase 06 read implementation before phase 04 issue 07 defines the access-path contract it is implementing.
