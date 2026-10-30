# Production Readiness

This document is the canonical owner for beta and Production-ready evidence. Feature behavior and status live in [Feature Support](feature-support.md). Passing unit or integration tests does not by itself establish either readiness classification.

## Current Classification

The Cassie query-engine baseline is a **Production Candidate** for the documented
unreleased 0.1.0 support envelope. Stable capabilities are supported;
Experimental capabilities are available for evaluation under their documented
limits and may change before 1.0.

Cassie is not Production-ready. Local disk-backed smoke evidence is sufficient to catch correctness and gross resource-bound regressions, but it is not representative-scale evidence for production latency, capacity, cancellation latency, recovery time, or sustained concurrency.

## Product release boundary

The [Product Roadmap](product-roadmap.md) defines v0.1.0 Projection Store,
v0.2.0 Query and Retrieval, and v0.3.0 Authoritative Columnar Tables as the
three feature releases. Production readiness is a separate qualification gate;
this document defines its evidence without creating another feature milestone
or expanding SQL and server compatibility scope.

PostgreSQL 18.6 comparisons qualify selected behaviors named by current
roadmap milestones and issue criteria. The former complete-core-SQL/full-PLpgSQL
program (#791–#793) was closed as superseded. Neither that program nor the size
of its retained test corpus is a production-readiness gate. Qualification is
owned by the [release checklist](release-checklist.md) and the production
evidence below; GitHub feature milestones remain limited to the three releases
in the roadmap.

## Prior Local Check Snapshot — 2026-10-29

This candidate-tree result predates the 2026-10-30 0.1.0 format reset. It does
not qualify the current baseline; repeat the ordered gate on the exact release
commit.

On 2026-10-29 the ordered local gate passed on the documentation scope
worktree `audit/v03-mvp-scope-20261029`, based on main
`3eb621c640cd1df0e95a43819fd4735c013659dd`:

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: 9,844 passed, 0 failed, 12 ignored across 35 targets.
- Strict workspace/all-target/all-feature pedantic Clippy: passed.
- `cargo fmt --all -- --check`: passed.
- No Rust test files were changed by this documentation scope slice.
- `cntryl-tools validate-docs`: passed for 126 Markdown documents.

This is local working-tree evidence, not validation on a frozen release
commit. Selected client workflows and exact-release-commit browser, benchmark,
and disk-backed operational evidence still need resolution. GitHub Actions are
disabled, so hosted checks and publication are unavailable.

The earlier [Local Development Readiness checkpoint](local-development-readiness.md)
records the same test count on a candidate working tree based on `75dd9269`,
without an exact run-tree SHA. PR #1115 subsequently merged its composite-index
restart test to main at `3eb621c640cd1df0e95a43819fd4735c013659dd`; its hosted
check rollup was empty while Actions were disabled.

### Historical main snapshot — 2026-10-19

The earlier local gate ran on `main` at base commit
`39523c963b744e375070c749ecb56a3897845c9e` with an uncommitted documentation
and test-fixture-label reconciliation. No runtime source changed.

- `cargo build --locked --bin cassie`: passed.
- `cargo test --locked`: failed in `tests/catalog_security.rs` with 279 passed
  and 123 failed. A focused reproduction of
  `type_catalog_discovery::should_advertise_pgcrypto_in_available_extension_catalog`
  confirms `pg_catalog.pg_available_extensions` is absent. This is planned
  #843 behavior. The remaining failures span backend/session statistics,
  server-control helpers, catalog shapes and size/visibility functions, and type
  discovery; they have not been individually mapped to source findings or issue
  owners. Do not attribute them to #843 based on this run alone.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic`:
  passed.
- `cargo fmt --all -- --check`: passed.
- `cntryl-tools validate-tests -f tests/sql_queries/types/uuid_lsn/uuid_hash_lsn.rs`:
  passed (36/36).

## Workflow and Branch-Protection State

GitHub readback on 2026-10-30 reports repository Actions permission `enabled: true`; CodeQL is
active, while `Containers`, `Version`, `CI Backend`, `CI Frontend`, and `Publish` are manually
disabled. No `Operational readiness` workflow is present in the current workflow tree or registry.
`AGENTS.md` still prohibits enabling or dispatching workflows until the user explicitly changes
that direction; no backend, benchmark, or release workflow was dispatched.

PR #1144 remains open at `92bf7c77babd5c24c08fdf7dafb861d8b0eb3fba`. CodeQL run
`41117159326` is analyzing this exact head: the Python, Actions, and JavaScript/TypeScript
analyzers passed, Rust was still running at the latest readback, and the umbrella CodeQL check was
skipped. This head includes the type-only diagnostic and regression for alert 522
(`rust/cleartext-logging`); its hosted result is pending completion of the Rust analysis. The only
recorded review is a GitHub Advanced Security bot comment; no human review or review decision is
recorded. This analysis does not establish backend test or release qualification.

The 2026-10-30 `main` branch-protection readback returned no applicable repository rules and the
branch-protection endpoint returned 404 (`Branch not protected`). There is no active `main` branch
protection. These observations should be reviewed before selecting a repository-wide protection
policy; no ruleset or protection setting was changed.

Historical coverage recorded on 2026-10-12 is retained as context only: backend
CI filtered out `ui/**`, frontend CI ran for frontend-related paths, and PRs
[#940](https://github.com/cntryl/cassie/pull/940) and
[#941](https://github.com/cntryl/cassie/pull/941) had successful backend checks.
Those path filters and results do not describe current hosted coverage or
active branch protection.

## Evidence Present

- The repository's locked build, full-test, pedantic Clippy, formatting, and
  touched-test validation gates are defined. The latest local worktree result is
  recorded in [Testing and Validation](testing/validation.md); it is not an
  exact-release-commit qualification.
- Integration coverage across SQL, indexes, transactions, pgwire, REST, search, vector, analytics, projection lifecycle, and recovery adapters.
- The stable named-client catalog subset is limited to the documented columns of
  `information_schema.tables` and `information_schema.columns`, with lifecycle tests plus retained
  passing sqlx 0.8.3 and Diesel 2.2.6 loopback pgwire manifests. Other virtual catalogs and
  PostgreSQL-internal parity remain Experimental.
- The Stable limited-procedure subset is confined to one persisted Cassie SQL statement with
  positional argument binding. Restart, deterministic unsupported-syntax, recursion,
  transaction-control, and `tokio-postgres` create/call/read evidence are retained; PL/pgSQL,
  triggers, dynamic SQL, and business-procedure workflows remain outside the product contract.
- Restart and generation-fencing coverage for multiple persisted derived artifacts.
- Tiered benchmark owners with environment-labelled local observations.
- Persisted full-text SQL evidence covering posting reads, exact BM25 equivalence, snippets, structured prefilters, bounded candidate row fetches, transaction overlays, corruption fallback, cancellation, and memory limits.
- Exact, HNSW, IVFFlat, and hybrid evidence covering bound parameters, persisted candidates, exact reranking, structured filters, deletion visibility, explicit fallback diagnostics, cancellation, hard memory limits, and at least 0.90 ANN recall on deterministic 10k and 100k disk-backed fixtures.
- The [HNSW Release Evidence](hnsw-release-evidence.md) contract fixes the representative fixture,
  graph parameters, exact-top-k recall comparison, resource observations, and interrupted-publication
  recovery proof. Its exact-head native-Linux artifact link remains open until the release-candidate
  run completes; the Stable feature label is not a Cassie-wide capacity claim.
- The [IVFFlat Release Evidence](ivfflat-release-evidence.md) contract fixes the representative
  list, probe, training, filter, recall, resource, and interrupted-publication matrix. Its exact-head
  native-Linux artifact link remains open until the release-candidate run completes.
- Deterministic local-server contracts for OpenAI, OpenAI-compatible, TEI, Ollama, Voyage, Cohere, and local embeddings, including request shape, ordering, dimensions, retry deadlines, transport timeouts, and active cancellation.
- Remote embedding-provider evidence is mock-provider evidence: it covers authentication headers,
  HTTP 429 handling, bounded retry attempts, query-deadline clamping, response limits, and
  deterministic failures. It does not establish hosted availability, provider quotas, model
  availability, service latency, or account-specific authorization. Operators must monitor the
  selected service and size retry counts and query deadlines to its published rate-limit policy.
- Security-boundary coverage for constant-cost authentication failures, bounded login state, atomic REST session quotas, explicit external HTTPS attributes, bounded provider responses, streaming transport deadlines, parser complexity budgets, and live database-access revocation.
- Health, metrics, EXPLAIN, projection diagnostics, capacity guidance, snapshot/restore guidance, and repair runbooks.
- Retained container and supply-chain evidence for supported targets from
  earlier runs. The 2026-10-30 local ARM64 image and restart smoke is recorded in
  [Testing and Validation](testing/validation.md); its digest is local-only and it
  does not satisfy exact-release-commit or amd64 evidence. No backend, benchmark,
  or release workflow has been dispatched under the current no-dispatch
  instruction; those required hosted results remain blocked.
- Named evidence boundaries are defined in [Deployment Profiles](deployment-profiles.md); the
  native-Linux profile remains an explicit evidence requirement rather than an implied claim.
- The release support, upgrade, rollback, and security-response envelope is defined in
  [Support, Release, and Security-Response Policy](support-policy.md).
- Bounded pull execution, portal streaming, cancellation, result-cache isolation and invalidation, compact row layout, specialized access paths, and shared worker-permit coverage.
- The [Query Promotion Evidence](query-promotion-evidence.md) inventory maps each optimized query
  family to an independent baseline and retained evidence. Seeded differential cases cover scalar
  pagination and overlays, native-versus-adjacency graph reads, join and CTE rewrites, window ties
  and null ordering, and insertion-order permutations. These close #11's evidence gaps without
  changing Experimental feature status; each family still requires its named feature-owner and
  operational promotion review.
- The selected opt-in adaptive surface is limited to feedback-informed scalar read selection and
  vectorized-to-merge inner/left equi-join switching. A retained backend-CI artifact records a same-commit
  `cassie-adaptive-profile-<commit>` artifact with fixed/evaluation results, plans, storage reads,
  candidates, memory, workers, fallback diagnostics, and configuration. Deterministic tests cover
  feedback guards and failure states plus restart rollback. Broader adaptive planning remains
  Experimental, and the default profile remains disabled.
- Fixed-duration time-series indexing is Stable for positive integer minute, hour, and day widths.
  The 15-minute, 1-hour, and 1-day differential matrix is backed by independent authoritative-row
  controls, and PR [#231](https://github.com/cntryl/cassie/pull/231) records the exact promotion
  commit and retained native-linux-amd64-disk artifact `cassie-benchmark-issue-27-time-series-pr231`.
  Rollups, retention workflows, calendar buckets, local-time boundaries, and daylight-saving
  semantics remain outside this promoted surface.
- The local materialized projection lifecycle is Stable as defined in
  [Materialized Projection Lifecycle](materialized-projection-lifecycle.md). PR
  [#233](https://github.com/cntryl/cassie/pull/233) records exact commit
  `815e962b3edaa2fe4cf1edb48fc2040b4aee0492` with retained native-linux-amd64-disk Tier 5
  artifacts for `perf.rebuild.refresh.100k` (run 35087618539), `perf.verification.full.100k`
  (run 35092899983), `perf.repair.projection_hashes.100k` (run 35092902383), and
  `perf.replay.lag_catchup.100k` (runs 35092897748 and 35095312063). Every measured sample
  completed all logical operations with zero failures, timeouts, drops, duplicates, validation
  errors, fallbacks, or worker leaks, and peak accounted query memory stayed within the 64 MiB
  budget. Refresh, verification, and repair throughput met acceptable or authoritative gate
  quality; replay throughput was noisy on hosted runners and is diagnostic only, not a replay
  latency or capacity objective. Column-store table mode, retained 250k capacity rows, and
  deployment-profile alert thresholds (#8) remain outside this promotion.
- Canonical v1 column-batch format and corruption tests, automatic typed codecs, selected-value dictionary and FSST decoding, encoded scan and filtered-aggregate parity, generation-fenced range copy-on-write DML, and paired Tier 2 codec acceptance gates. ALP codec version 1 has the retained `should_emit_cross_architecture_stable_alp_bytes` golden fixture, sparse-null and exact-result fallback/rebuild coverage, unknown-version fail-closed coverage, and paired `perf.column.alp_selective_scan.2k` / `perf.column.alp_plain_scan_baseline.2k` owners enforcing at least 75% chunk-byte savings and no more than 5% p95 query regression. FSST codec version 1 has the retained `should_emit_cross_architecture_stable_fsst_bytes` full-CBC1 golden fixture plus sparse-null selected decoding, corrupt-unselected-value rejection, fallback/restart rebuild, and rename/drop lifecycle coverage. Its paired `perf.column.fsst_selective_scan.2k` / `perf.column.fsst_plain_scan_baseline.2k` owners require every candidate chunk to save at least 25% and the selective-query p95 to remain within 5% of forced plain. Each of the four candidate/baseline pairs owns an independent sampling group and alternates within each invocation so unrelated pairs cannot create sustained drift or bias the comparison.
- Locked UI install, production-dependency audit, generated-client freshness, tests, type checking, lint, and production build.
- Monaco sanitizer resolution is controlled by the scoped override in
  [`ui/package.json`](../ui/package.json). Release qualification requires a fresh
  locked production-dependency audit and the UI gates; dated resolved-version
  evidence is recorded in [Validation](testing/validation.md). Monaco's vendored
  import is routed to the audited dependency in production, development
  optimization and worker builds. Upstream dependency
  tracking remains [microsoft/monaco-editor#5352](https://github.com/microsoft/monaco-editor/issues/5352).
- Retained production-browser evidence records the Admin UI running against a real temporary Cassie process at desktop and mobile viewports. The Askr `0.4.4`, Askr UI `0.4.3`, `@askrjs/themes` `0.4.2`, and `@askrjs/monaco` `0.4.0` graph with Monaco `0.56.0` and DOMPurify `3.4.17` passed 121 repository tests, 26 desktop/mobile mock-browser cases, and 2 real-Cassie desktop/mobile production-browser cases together. Five repository tests qualify sanitizer resolution in production, dependency optimization and worker graphs, importer isolation and missing-dependency rejection; dated evidence is recorded in [Validation](testing/validation.md). Mock-browser evidence covers axe scans of login, empty workspace, controlled dialog, populated results, query failure, and mobile-sidebar states plus committed populated desktop light/dark and mobile open/closed screenshots. The production-browser case loads a fresh Monaco editor, immediately filters the schema tree, and rejects the historical `state.set() cannot be called during component render` console failure. Cassie isolates live query-text reactivity from Monaco host reconciliation so editor selection, history, completion, results, and theme changes preserve workspace state. The released packages resolve [askrjs/askr-monaco#22](https://github.com/askrjs/askr-monaco/issues/22) and [askrjs/askr-themes#62](https://github.com/askrjs/askr-themes/issues/62), so their application workarounds are removed. This evidence promotes only the Admin UI support entry to Stable; Cassie's overall Production Candidate classification is unchanged.
- Askr does not yet expose a lifecycle-owned dynamic keyed-query collection with bounded prefetch and aggregate state. Cassie therefore retains one narrow `DatabaseCatalogController` for lazy schema loading, three-request search prefetch, retry, aggregate progress, and abort-on-unmount without adding another schema cache. The upstream capability is tracked in [askrjs/askr#327](https://github.com/askrjs/askr/issues/327).

## Rollup and Retention Operator Contract

The durable expression and recovery contract is in [Rollup Lifecycle](rollup-lifecycle.md).

Rollup refresh and retention enforcement are explicit, local, single-node maintenance commands.
Run `ENFORCE RETENTION POLICY <policy> AT '<timestamp>'` with a recorded cutoff, inspect the
policy catalog row and runtime metrics, then run `REFRESH ROLLUP <rollup>` when an explicit
refresh is required. Repeating either command over unchanged state is supported: subsequent
retention windows delete zero additional eligible rows, and repeated rollup refreshes preserve
the same aggregates and source generation.

The stable metric keys and their cumulative semantics are:

- `retention.enforcements`, `retention.deleted_rows`, `retention.skipped_rows`, and
  `retention.errors` count completed enforcement attempts, deleted source rows, rows skipped
  because they were invalid or referentially restricted, and failed attempts. The gauges
  `retention.last_policy` and `retention.last_error` identify the most recent policy and failure.
- `rollups.refreshes`, `rollups.rewrite_hits`, `rollups.fallback_scans`, and
  `rollups.stale_fallbacks` count completed refreshes, rollup-backed reads, authoritative-source
  fallbacks, and the stale subset of those fallbacks. The gauges `rollups.last_rollup` and
  `rollups.last_fallback_reason` identify the latest rollup activity.
- `pg_catalog.pg_maintenance_debt` is the durable recovery surface. `artifact`,
  `target_generation`, `retry_count`, `last_error`, and `fallback_reason` describe work that must
  be replayed before the derived artifact can serve reads.

Cancellation or resource exhaustion before durable source mutation returns an error and must not
be reported as successful enforcement or refresh. A failure after source mutation leaves the
source authoritative, records maintenance debt, and forces rollup reads to fall back until retry
completes. Restart automatically retries durable rollup debt. If debt remains or `retry_count`
continues increasing, preserve the database directory and diagnostics, correct the underlying
storage or resource condition, restart, and verify the debt row disappears before relying on the
rollup again.

The retained capacity owners are `perf.time_series.retention.100k` and
`perf.time_series.rollup_refresh.100k` on the `native-linux-amd64-disk` profile. Their artifacts
separate fixture setup from measured work and retain affected rows, storage activity, peak query
memory, configured workers, variance, and trust status. These 100k results are representative
evidence, not an SLO or an assertion that larger operator windows fit the same maintenance window.
Calendar-aware buckets, background scheduling, distributed coordination, and analytical
projection lifecycle remain outside this contract.

## Production Candidate Support Envelope

- PostgreSQL wire is the primary SQL interface; REST is secondary and administrative.
- Only capabilities marked Stable are supported contracts. Experimental capabilities are evaluation surfaces, not compatibility commitments.
- Midge is the only storage layer. Cassie is permanently single-node and does not provide distributed SQL, cluster management, replication, consensus, sharding or rebalancing, cross-node transactions, distributed planning, remote query forwarding, or automatic cross-node repair.
- The Production Candidate bar requires the validation sequence in [Definition of Done](definition-of-done.md), UI production-dependency audit and gates, benchmark-owner compilation, and a disk-backed smoke run on the release commit.
- Smoke results are regression diagnostics, not service-level objectives or capacity claims.

## Remaining Production Blockers

Tracking owners #8, #29, and #781 were closed as `NOT_PLANNED` on 2026-10-28.
Their closure comments say this did not complete the acceptance criteria or
resolve the outstanding evidence gaps. The blockers below remain active under
the roadmap's separate production-readiness boundary.

- Database-image checksums detect content changes but do not authenticate who produced an image.
  Operators must follow the detached-signature and trusted-identity procedure in
  [Snapshot, Backup, Restore, and Repair](snapshot-restore.md) before streaming an image from an
  untrusted channel into `RESTORE`; Cassie intentionally does not define another signing format.

- Retain complete same-commit Tier 1-6 artifacts for
  `workstation-apple-m5-arm64-apfs`, including the two-hour Tier 6 set, and add
  representative native-Linux capacity and soak evidence before a global Production-ready
  claim.
- Establish and validate operational thresholds for disk growth, resource admission, backup/restore time, rebuild and repair time, failure injection, cancellation latency, and sustained mixed workloads.
- Exercise container startup, health, restart, snapshot, restore, and failure-recovery runbooks in each supported release architecture and deployment profile.
- The cross-architecture rehearsal record is defined in [Cross-Architecture Release Rehearsal](cross-architecture-rehearsal.md); amd64 and arm64 evidence remains pending until separate retained bundles exist.
- The historical `Operational readiness` workflow from PR #226 is absent from the current
  `.github/workflows/` tree and GitHub workflow registry. Its shape and scale/endurance modes are
  not runnable in this checkout. The repository permission reports Actions enabled, but `AGENTS.md`
  prohibits dispatch; the required retained native amd64/arm64 evidence remains pending.
- Validate the support, upgrade compatibility, release rollback, and security response expectations
  in [Support, Release, and Security-Response Policy](support-policy.md) against a tagged release
  artifact and rehearsal evidence.

## Promotion Evidence

A Production-ready claim must link to the exact commit, toolchain, deployment profile, configuration, fixture, complete owner-suite artifacts, restart or recovery evidence, resource-bound measurements, and known limitations. Evidence must include result correctness, selected access paths, fallback reasons, storage reads, candidates, peak accounted query memory, workers, and cancellation latency. Local fallback-storage results remain developer diagnostics.

Midge evidence owns persistence, durability, and recovery mechanics. Cassie readiness evidence owns logical layout compatibility, query-visible failure behavior, adapter integration, restart hydration, and query semantics over recovered data.

## Query Engine Target 1 Promotion Boundary

[Query Engine Target 1](query-engine-target.md) separates the finite PostgreSQL
wire/dialect/client, physical ColumnStore, CBM1 acceleration, typed relational
execution and embedding-vector retrieval axes. The
[invariant ownership ledger](query-engine-invariant-ownership.md) maps all baseline
obligations to child acceptance and existing operational owners.

The [finite type contract](type-contract.md) pins compatibility and explicit wire
exclusions. Its local and hosted qualification remains separate from typed
native adoption and release promotion.

This foundation does not promote a capability or change Production Candidate to
Production-ready. Mapped tests, fixed P0 defects, a completed milestone, trace
replay and short smoke runs remain distinct from retained native qualification.
[#8](https://github.com/cntryl/cassie/issues/8) retains native profile objectives,
measurement windows, repeated-run variance, recovery/scale/soak and fail-closed
artifact requirements. [#29](https://github.com/cntryl/cassie/issues/29) retains local
diagnostic promotion after that evidence; its already delivered local field
contracts are not duplicated. [#781](https://github.com/cntryl/cassie/issues/781)
consolidates the final exact revision/image/profile/client/known-limit ledger only
after its dependencies complete. Existing deployment profiles and evidence
manifest formats remain unchanged; no self-hosted runner work is introduced.
