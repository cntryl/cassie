# Cassie Remaining Work

Audit date: 2026-07-11

This is the ordered implementation, test, benchmark, documentation, and delivery backlog for
Cassie. It is grounded in the current repository, `docs/product-roadmap.md`,
`docs/production-readiness.md`, `docs/experimental-promotion-criteria.md`, current tests and
benchmarks, and the active remediation phases. Existing broad feature labels do not close an item:
the item is complete only when its implementation, deterministic failure behavior, restart safety,
tests, diagnostics, documentation, and performance evidence meet `docs/definition-of-done.md`.

## Working rules

- Work in the phase order below. Do not start a later phase while an earlier required dependency is
  open.
- Write a focused failing `should_...` test first with `// Arrange / Act / Assert`.
- Keep Midge as the only storage layer and pgwire as the primary query interface.
- Use the clean-break lexkey v5 layout only. Do not add legacy readers, migrations, or compatibility
  ladders for old Cassie storage/snapshot formats.
- Keep source and test files below 1,000 lines. Split an oversized touched file before adding
  material behavior.
- Close each slice in this order:
  1. `cargo build --locked`
  2. `cargo test --locked`
  3. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`
  4. `cargo fmt --all -- --check`
  5. `cntryl-tools validate-tests -f <path>` once per touched test file
- Do not promote a feature from broad integration coverage alone. Promotion requires the exact
  evidence in `docs/experimental-promotion-criteria.md`.

## Current baseline that should not be reimplemented

- Atomic row/index/vector-family writes, constraint enforcement, read-only non-admin roles, and
  loopback-safe default credentials have landed.
- Derived vector state, normalized vectors, cardinality statistics, column batches, and projection
  hashes have generation/restart protection. Index publication and collection/field rename/drop
  operations have durable replay coverage.
- Snapshot format v2 records schema/data epochs and collection generations and rejects incompatible
  formats.
- SQL parsing/execution, recursive CTEs, windows, transactions, pgwire simple/extended query, REST,
  OpenAPI adapter generation, vector indexes, analytics, metrics, and the Tier 1-4 cntryl-stress
  benchmark suite all have baselines. The tasks below close correctness or production-depth gaps in
  those baselines; they do not restart them.
- Distributed SQL, replication, consensus, cross-node reads, automatic fleet movement, full
  PostgreSQL parity, triggers, and a stored-procedure business-logic platform remain non-goals.

## Phase 0 — trustworthy gates and contract claims

- [x] Complete add-column recovery coverage and retain final gate evidence.
  - Baseline: `ALTER TABLE ... ADD COLUMN` now persists column-batch and projection-hash maintenance
    debt before post-commit refresh and retries it on startup.
  - Tests: keep the projection-hash interruption/restart regression in
    `tests/derived_state_recovery.rs`; add column-batch debt coverage when a column index exists.
  - Benchmark: N/A; record maintenance retry/fallback metrics instead.
- [x] Reconcile current documentation with source truth.
  - Document the current v2 manifest contract and explicit v1/non-v2 rejection in
    `docs/snapshot-restore.md` and the recovery row in `docs/feature-support.md`.
  - Keep lexkey v5 current-layout and v4-and-older rejection wording consistent across README,
    feature support, snapshot docs, tests, and startup diagnostics.
  - Narrow any Stable/Implemented claim whose remaining contract work is listed below, especially
    NULL semantics, recursive CTEs, window frames, binary pgwire, retrieval generation safety,
    REST authentication, analytics freshness, and production readiness.
- [x] Make gate results reproducible and retained.
  - Ensure the full locked test suite completes deterministically in local and CI environments.
  - Upload failed-test output and relevant diagnostics in CI so a gate failure is actionable.
  - Keep UI adapter freshness, UI test/type/lint/build, bench compile, Rust build/test/fmt/clippy, and
    test-convention validation in the normal CI path.

## Phase 1 — atomic write and constraint correctness

The deterministic baseline is implemented. Remaining randomized/model coverage is tracked in
Phase 9. Do not widen this phase into general OLTP or distributed transaction work.

## Phase 2 — derived-state publication and crash recovery

- [x] Close every current base-write-plus-derived-refresh boundary with “commit plus durable
  stale/debt mark,” never an ambiguous failed write after base data is durable.
  - Audit scalar/time-series/full-text/vector/graph/column-batch/rollup/analytical/hash paths.
  - Persist debt or invalid generation in the same base-data transaction where possible.
  - Retry idempotently at startup and expose retry count, last error, target generation, and current
    fallback reason.
  - [x] Scalar, time-series, vector, and graph sidecars publish in the same Midge data transaction
    as their source rows and carry the source collection generation; family failpoint tests reject
    partial publication and generation tests fence old state.
  - [x] Column-batch and projection-hash refreshes use the shared generation-bound debt contract,
    startup replay, maintenance-pending fallback, and redacted retry diagnostics.
  - [x] Rollup writes now use a source-scoped `rollup` debt record, a `maintenance_pending` read
    fence, catalog diagnostics, and startup retry/clear coverage.
  - [x] Materialized and analytical source writes now persist a generation-bound
    `materialized_projection` debt in the base-write transaction, fence reads while stale marking
    is pending, and replay stale metadata idempotently after restart with retry/error diagnostics.
  - [x] Full-text currently rebuilds its in-memory query-time index from authoritative rows and has
    no independent post-commit persisted refresh boundary; persisted postings/statistics and their
    publication debt remain the explicitly scoped Phase 5 retrieval slice.
- [ ] Complete schema-operation journal coverage.
  - Verify create/drop/rename collection, add/drop/rename field, and create/drop index behavior at
    every schema/data-family interruption point.
  - Ensure prepared operations are never query-visible, replay is idempotent, abandoned validation
    intents are discarded safely, and cleanup leaves no orphaned index/sidecar keys.
  - Add crash/failpoint and concurrent-write tests for each cross-family boundary.
  - [x] Drop-collection replay now finishes data cleanup after an interrupted schema commit and is
    safe across a second restart.
  - [x] Collection-rename journals are published only after validation and move maintenance debt
    alongside generation and other collection-prefixed state.
  - [x] Collection-rename replay preserves an already-committed destination write when a
    same-key source row is still present after an interrupted schema commit.
  - [x] Prepared vector index publication keeps the in-memory vector registry hidden until the
    generic index metadata commit succeeds, then rehydrates both records on restart replay.
  - [x] Prepared column index publication rebuilds column batches before generic metadata becomes
    visible, including restart replay after an interrupted publication.
  - [x] Drop-index cleanup removes scalar/time-series sidecars before metadata and retries safely
    after an injected metadata-interruption failure.
  - [x] Add-column journals publish the schema commit before generation-bound column-batch and
    projection-hash debt, replay both artifacts idempotently after restart, and clear the intent
    only after derived state is current.
  - [x] Dropping a graph backing collection removes its outbound/inbound adjacency sidecars during
    replay, including after an interrupted schema commit, without leaving orphaned edges.
  - [x] Field rename and drop operations now hold the collection write gate through schema journal
    publication and derived-sidecar replay, serializing them with concurrent DML.
  - [x] Collection rename now holds the source collection write gate through schema journal
    publication and derived-sidecar replay, serializing it with concurrent DML.
  - [x] Collection drop holds the collection write gate through schema metadata publication and
    data-family cleanup, serializing it with concurrent DML.
  - [x] Index drop holds the collection write gate through sidecar cleanup and metadata removal,
    serializing it with concurrent DML.
- [x] Make snapshot capture consistency executable, not documentation-only.
  - [x] Test a source mutation during copy and require retry/failure without leaving a usable
    partial snapshot.
  - [x] Validate restored per-collection generations, schema/data epochs, journal/debt state, and
    query results before accepting the restore.
  - [x] Add a true concurrent snapshot/write test; deterministic source-mutation rejection and
    interrupted-copy cleanup are covered below.
  - [x] Failed snapshot and restore copies remove partial directories before returning errors.
- [x] Resolve the planned “Merkle integrity index” row in `docs/feature-support.md`; the current
  contract is the existing persisted row/range/projection-root hash state, not a separate Merkle
  index.
- [x] Complete safe executable repair for projection scopes, including larger-manifest
  verification evidence.
  - Keep repair local, admin-only, audited, idempotent, rollback-aware, and post-verified.
  - [x] Row/range projection-hash repair takes the collection write gate, so concurrent writes
    cannot publish a repair from an obsolete source snapshot.
  - [x] A 1,024-row, four-range manifest is verified, repaired, exported with row hashes, and
    reopened after restart with the generation-bound root intact.
  - [x] Index scope now repairs verified column-batch sidecars under a collection write gate,
    records an audit report, requires post-verification, and survives restart.
  - [x] Full-rebuild scope now refreshes an active materialized projection only after a
    repairable full integrity report, gates source/output collections, records an audit report,
    requires post-verification, and survives restart.
  - [x] Projection-version scope now rebuilds an explicitly verified materialized version under
    source/output gates, preserves activation state, records an audit report, requires
    post-verification, and survives restart.
  - [x] Snapshot rollback rehearsal restores the pre-repair version state, query results, and
    repair-audit absence after a repaired version is rolled back to a local v2 snapshot.

## Phase 3 — stable SQL semantics

- [ ] Implement PostgreSQL-like three-valued NULL logic in `src/executor/filter.rs` and every join
  path.
  - Propagate unknown through comparisons, arithmetic, `LIKE`, `BETWEEN`, `IN`, and `NOT IN`.
  - Implement the complete `AND`/`OR`/`NOT` truth tables; null equality keys must not join under `=`.
  - Add binder-time rejection of incompatible comparison/arithmetic operand families.
  - Return a typed division-by-zero error and SQLSTATE `22012` instead of `0.0`.
  - Tests: add `tests/integration_sql_null_semantics.rs` for truth tables, predicates, arithmetic,
    list/null behavior, and merge/vectorized/fallback join equivalence; add pgwire `22012` coverage.
  - Benchmark: N/A beyond regression checks; correctness is the gate.
- [ ] Complete constant and parameter-only `SELECT`.
  - Accept supported scalar literals, expressions, aliases, booleans, NULL, and bare/cast parameters
    without `FROM` through parser, binder, planner, and `QuerySource::SingleRow` execution.
  - Replace hard-coded float metadata for expression projections with expression-aware result types.
  - Preserve explicit/inferred parameter OIDs through prepared-statement Describe.
  - Tests: parser/engine/pgwire coverage for `SELECT 1`, strings, booleans, NULL, aliases,
    expressions, `SELECT $1::INT`, explicit-OID `$1`, and table-free set operations.
  - Benchmark: no dedicated benchmark required; include the path in protocol microbench coverage.
- [ ] Correct recursive CTE working-table semantics.
  - Carry `UNION` versus `UNION ALL` in the AST.
  - Feed only the previous iteration’s delta into the recursive term.
  - Deduplicate only `UNION`; preserve duplicates for `UNION ALL`.
  - Apply CTE aliases and validate anchor/recursive arity and compatible types.
  - Reject anchor self-reference, multiple unsupported recursive references, and unsupported shapes
    deterministically.
  - Tests: bounded `1..N`, UNION deduplication, UNION ALL duplicates, aliases, params, type/arity
    errors, self-reference errors, depth/temp-memory limits, and tokio-postgres end-to-end behavior.
  - Benchmark: 10k/100k bounded recursive workloads with depth, memory, and cardinality assertions.
- [ ] Define and implement the window-frame contract.
  - Add explicit frame AST/parser representation and ordered default-frame behavior.
  - Support documented `ROWS` bounds and apply frames correctly to `first_value`/`last_value`.
  - Keep ranking/offset functions frame-independent.
  - Reject `RANGE`, `GROUPS`, `EXCLUDE`, invalid bound order, and negative offsets with deterministic
    `0A000` until supported.
  - Tests: default, whole-partition, bounded preceding/current/following, peers, empty/single-row
    partitions, invalid/rejected frames, and pgwire errors.
  - Benchmark: 10k/100k partitioned frame workloads with memory/result checks.

## Phase 4 — transactions and pgwire contract alignment

- [ ] Enforce the single-collection transaction limit at the second staged collection, not only at
  COMMIT.
  - Make `CassieSession::stage_document_write/delete` fallible.
  - Reject the second collection with `CassieError::Unsupported`/SQLSTATE `0A000`, mark the
    transaction failed, preserve existing staged state for rollback, and preflight cross-collection
    foreign-key cascades.
  - Tests: rejection timing, rollback recovery, no partial mutation, pgwire SQLSTATE, and
    ReadyForQuery `E/T/I` status.
- [ ] Reject transaction semantics Cassie does not provide.
  - Accept only the documented default/read-committed contract.
  - Reject SERIALIZABLE, REPEATABLE READ, `SET TRANSACTION`, DDL in active transactions, and
    unsupported COPY/foreign-key cascade shapes before any catalog/data mutation with `0A000`.
  - Tests: every schema/index/view/projection DDL family, COPY, isolation levels, cascades, and no
    partial state.
- [ ] Fix the irreversible COMMIT boundary.
  - Once base writes commit, clear committed session state before any fallible derived refresh.
  - Persist stale/maintenance debt for rollup or other derived-refresh failure instead of returning
    an apparently retryable COMMIT that can duplicate a durable write.
  - Tests: injected post-commit refresh failure, retry/rollback behavior, restart recovery, and no
    duplicate writes.
- [ ] Implement quote/comment-aware pgwire simple-query multi-statement execution in a focused
  module.
  - Split on real statement separators while preserving semicolons in strings, quoted identifiers,
    line comments, and block comments.
  - Execute in order, emit each result sequence, stop on first error, ignore empty statements, and
    emit exactly one final ReadyForQuery.
  - Define deterministic handling for COPY mixed into a multi-statement batch.
  - Tests: ordered result sets, DDL+DML+SELECT, empty statements, quoted/comment semicolons,
    stop-on-error, transaction batches, COPY rejection, and frame counts.
  - Benchmark: Tier-4 real-transport 10k/100k multi-statement batches.
- [ ] Finish binary pgwire formats.
  - Never advertise binary while sending text fallback bytes.
  - Add exact result and parameter codecs for every advertised OID, especially UUID and
    date/time/timestamp.
  - Validate requested formats against prepared result schemas at Bind/Describe.
  - Reject unsupported binary vector/array/other representations with `0A000`.
  - Tests: byte-level DataRow coverage for null, bool, all integer widths, float8, bytea, UUID,
    date/time/timestamp, text-compatible types, mixed formats, and unsupported representations.
  - Benchmark: Tier-4 10k/100k binary parameter/result round trips.
- [ ] Complete common SQLSTATE coverage and documentation for all reachable unsupported paths.

## Phase 5 — persisted retrieval in lexkey v5

- [ ] Persist full-text retrieval state in Midge.
  - Store postings, term frequencies, document lengths, and corpus statistics with a built
    generation; stop rebuilding an `InvertedIndex` from every row on every query.
  - Maintain it atomically on insert/update/delete and index create/drop/rename/rebuild.
  - Add durable publication/debt replay, corruption detection, deterministic row-scan fallback,
    and stage metrics.
  - Tests: restart, mutation, cleanup, interrupted publication, corrupt postings, old-generation
    rejection, and exact fallback equivalence.
- [ ] Make ANN storage reads genuinely bounded.
  - Replace full-prefix normalized-vector validation and monolithic HNSW graph reads with an
    addressable generation manifest plus point-readable graph nodes/list membership/candidates.
  - Point-read only selected candidates and exact-rerank against current source-row vectors.
  - Fall back deterministically when a candidate/vector is missing or its generation changes.
  - Tests: read-counter scaling, concurrent generation changes, missing candidates, and equivalence
    between candidate generation and exact source rerank.
- [ ] Replace hybrid full-row prefilter/merge with bounded persisted text and vector candidate
  streams, structured prefilter pushdown, explicit budgets, and exact final source-row scoring.
  - Tests: stale text/vector/both, selective filters, caps, truncation, and fallback equivalence.
- [ ] Add retrieval-stage diagnostics: posting reads, ANN node/list reads, generation rejection,
  exact rerank count, truncation, candidate budgets, and fallback reason.
- [ ] Benchmark retrieval truthfully.
  - Cold/warm full-text at 10k, 100k, and a larger corpus with reads, memory, write amplification,
    and rebuild cost.
  - HNSW `ef_search` and IVFFlat lists/probes recall@k versus exact, latency, candidate reads,
    restart, refresh-after-write, and concurrency.
  - Hybrid selectivity/budget/concurrency proving no full-corpus merge/rerank.

## Phase 6 — scoped REST, TLS, opaque sessions, and UI authentication

- [ ] Bind every REST request to an authenticated database/schema session.
  - Stop global suffix resolution in collections/documents/indexes/search handlers.
  - Reject ambiguous names and scope listing/catalog/resource access to the request database and
    search path.
  - Tests: duplicate names across databases/schemas and admin/read-only authorization on every
    resource route.
- [ ] Replace `Bearer <user>:<password>` and browser `localStorage` credentials with opaque server
  sessions.
  - Add login/current-session/logout endpoints, random tokens, expiry, revocation, bounded session
    storage/cleanup, and `HttpOnly`/`Secure`/`SameSite` cookies.
  - Remove password/token persistence and per-request password headers from the UI.
  - Add global 401 handling, session bootstrap after reload, and server-backed logout.
  - Tests: invalid login, expiry, revocation, password rotation/role deletion, cookie flags, session
    caps, reload, redirect, and logout.
- [ ] Add inbound REST TLS with rustls configuration and fail-closed non-loopback policy.
  - Tests: valid HTTPS, missing/invalid key/certificate, and plaintext policy.
- [ ] Bound and harden HTTP.
  - Limit body/header sizes; add header/read/body/idle/request timeouts and slowloris protection.
  - Enforce content types/methods, explicit same-origin/CORS and CSRF policies, CSP,
    `X-Content-Type-Options`, frame/referrer policy, HSTS on TLS, no-store for auth/API, and immutable
    caching for hashed assets.
  - Decide/document whether `/metrics` is public; test all probe/public boundaries.
- [ ] Update `public/openapi.yml` for scoped identities, cookie auth, sessions, limits, TLS/CSRF,
  and errors; regenerate `ui/src/adapters/generated/api.ts` and retain the drift gate.
- [ ] Refresh Tier-4 HTTP evidence over the final authenticated TLS/session stack: login/session
  lookup, handshake versus keep-alive, query/document/search, body rejection, and admission load.

## Phase 7 — analytics, recovery, capacity, and embedding resilience

- [ ] Make time-series state generation-safe and range-addressable.
  - Add collection generation to bucket records/manifests.
  - Encode bucket/time bounds for range scans and point-fetch matching rows instead of full index and
    row-prefix scans.
  - Tests: mutation/delete/retention, old generation, restart, concurrent rebuild/write, and bounded
    read counters.
- [ ] Bind rollup and analytical projection readiness to exact source collection generation(s), not
  global data epoch/row count or `ProjectionFreshness::Fresh` alone.
  - Persist stale/debt in the base transaction and replay at startup.
  - Tests: failpoint-after-base-commit, multi-source generation mismatch, restart, and no stale
    serving.
- [ ] Add local work admission beyond socket counts.
  - Bound query/blocking workers, embedding requests, rebuild/index builds, and expensive admin
    operations; expose queue depth, wait, rejection, cancellation, and permit metrics.
  - Keep placement/movement external; this is local resource admission only.
  - Tests: saturation, deterministic rejection, release after error/cancellation, fairness,
    shutdown, and metrics.
  - Benchmarks: load curves that show saturation knee, bounded memory/queue depth, rejection, and
    recovery.
- [ ] Harden every embedding provider consistently.
  - Honor `Retry-After`; use capped exponential backoff with jitter and one overall deadline.
  - Normalize retryable status/connect/timeout classification and bound provider concurrency.
  - Add metrics for request/batch latency, cache hits, retry reason/count, throttle, timeout,
    malformed response, rejection, and exhaustion.
  - Tests: 429, 5xx, connect errors, timeout, malformed payload, exhaustion, concurrency, total
    deadline, and never caching failures/dimension mismatches/stale provider config.
  - Docs/bench: rate-limit/concurrency/timeout guidance and deterministic local/mock batch/cache/
    concurrency evidence; hosted-provider data is operational evidence, not an SLA.
- [ ] Collect larger analytical evidence for column batches, aggregate acceleration, rollups,
  retention, analytical projections, and time-series widths, including fallback, freshness,
  rebuild, capacity, and generation-check overhead.

## Phase 8 — truthful benchmarks and promotion evidence

- [ ] Capture required evidence values in benchmark artifacts.
  - `PerformanceBenchmarkScenario` evidence names must resolve to before/after metrics, EXPLAIN/access
    path, fallback counts, cache/storage/capacity values, and correctness assertions.
  - Fail an owned scenario when required evidence is absent or wrong; do not print labels alone.
- [ ] Define checked regression policy.
  - Add per-scenario budgets or approved baselines; current artifacts contain empty `budgets` and
    optional baselines only.
  - Test threshold enforcement and require reviewed baseline updates.
- [ ] Add benchmark execution and retention workflows.
  - Run a stable lightweight CI subset and scheduled/manual full Tier 1-4 suites.
  - Upload raw JSON and rendered reports tied to commit, toolchain, host, storage/profile, and config.
- [ ] Repair report tooling/docs drift.
  - Implement the documented filesystem artifact validation/report command or correct the docs.
  - Replace/archive historical Criterion claims with current cntryl-stress evidence.
- [ ] Add bidirectional manifest/artifact checks.
  - Prove every manifest scenario has a runnable case, every optimization row has an owner, required
    10k/100k pairs exist, and emitted rows match declared metadata.
  - Reject accidental informational demotion of promotion rows.
- [ ] Resolve current signal debt.
  - Current local artifacts contain 135 rows: 82 optimization, 53 informational, and 15 warning
    diagnostics. Own, prune, stabilize, or explicitly document every informational/warning row.
  - Restore stable promotion eligibility for intentionally demoted projection refresh/verify,
    retention, and rollup scenarios before using them as gates.
- [ ] Define at least one canonical disk-backed deployment profile with host shape, toolchain,
  commit, cache state, concurrency, resource/config/adaptive thresholds, repeatability, and rollback
  policy. Keep local in-memory profiles non-SLA.
- [ ] Close surface-specific evidence: ANN recall, time-series bucket widths, analytical freshness/
  capacity, embedding resilience, and admission load.
- [ ] Add a tested promotion checklist requiring exact tests, artifacts, restart coverage,
  diagnostics, docs, deployment profile, and explicit unresolved blockers per selected surface.

## Phase 9 — delivery, supply chain, fuzzing, and maintainability

- [ ] Make container publication CI-qualified.
  - Do not publish arbitrary workflow-dispatch refs without a passing CI identity.
  - Use protected tag/release/environment approval; smoke-test health, pgwire, REST, and persistence
    on amd64 and arm64 before promoting the exact digests.
- [ ] Make images immutable and traceable.
  - Add commit-SHA tag and OCI source/revision/version labels; sanitize branch tags; verify all tags
    resolve to the expected digest; retain digest outputs and rollback instructions.
- [ ] Produce and verify supply-chain artifacts.
  - Generate SBOMs, checksums, provenance attestations, signatures, vulnerability scans, and a
    published artifact inventory.
  - Add cargo advisory/license policy and npm audit policy; use least-privilege `id-token` only for
    attestation.
- [ ] Pin mutable build inputs.
  - Pin GitHub Actions by commit SHA, Docker bases by digest, and cargo-chef/tool installers by
    explicit version; add reviewed automated dependency updates.
- [ ] Add reproducible container/package checks: image contents, non-root user, ports/config,
  startup, snapshot restore, release metadata, and failure-safe rollback/release notes.
- [ ] Add fuzz/property/model testing.
  - Targets: lexkey/key encoding, row/value/JSON round trips, parser non-panics, pgwire frames,
    snapshot/journal decoders, and transaction/schema/index recovery state machines.
  - Add bounded randomized concurrency models for uniqueness, foreign keys, commit/rollback,
    schema publication/replay, index generation, and admission permits.
  - Retain seeds/crash reproducers; add Miri/sanitizer coverage where practical.
- [ ] Enforce the 1,000-line rule with a repo-native CI ratchet.
  - Split current violations by domain before related feature growth:
    `tests/integration_sql_transactions.rs`, `src/sql/ast.rs`,
    `src/midge/adapter/documents.rs`, `src/app/query.rs`,
    `tests/integration_sql_constraints.rs`, `src/app/documents.rs`, and
    `src/executor/execution/scored.rs`.
  - Proactively extract near-limit orchestration modules rather than increasing the ratchet.
- [ ] Qualify both delivery architectures in CI, including the real disk-backed storage mode used
  for production claims.

## Compatibility and ecosystem follow-on backlog

These are in-scope compatibility probes, not permission to add client-name detection or full
PostgreSQL parity.

- [ ] Add automated sqlx, diesel, sea-orm, asyncpg, pgx/database-sql, Npgsql, JDBC, Postgrex, and
  libpq baseline probes for supported read-model workflows.
- [ ] Expand Prisma/SQLAlchemy/pgAdmin4 and migration-tool dry runs only where generated SQL/catalog
  queries stay inside the documented Cassie surface.
- [ ] Add migration/reflection coverage for composite constraint fidelity, identity/defaults,
  sequence metadata, supported ALTER TABLE breadth, and deterministic rejection of deferrable/
  extension/storage-specific behavior.
- [ ] Keep ecosystem suites opt-in or external when dependencies are heavyweight or non-deterministic;
  record exact Cassie and client versions and report failures as PostgreSQL-contract gaps.

## Final closure checklist

- [ ] Every item above is either completed with linked evidence or explicitly moved out of scope in
  the governing product docs.
- [ ] `docs/product-roadmap.md`, `docs/feature-support.md`, `docs/postgres-compatibility.md`,
  `docs/production-readiness.md`, and `docs/experimental-promotion-criteria.md` agree with code and
  tests.
- [ ] All persistent state has restart, rename/drop, cleanup, stale-generation, corruption, and
  interrupted-publication coverage appropriate to the surface.
- [ ] All protocol/API unsupported paths return deterministic HTTP/SQLSTATE errors.
- [ ] Performance-sensitive surfaces have traceable, truthful deployment-profile evidence and no
  unexplained informational demotion or warning diagnostics.
- [ ] Release artifacts are CI-qualified, signed/attested/scanned, multi-architecture tested, and
  reproducibly tied to source.
- [ ] The required Rust/UI/test/bench/adapters/module-size/fuzz/supply-chain gates are green.
