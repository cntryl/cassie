# Production Readiness

This document is the canonical owner for beta and Production-ready evidence. Feature behavior and status live in [Feature Support](feature-support.md). Passing unit or integration tests does not by itself establish either readiness classification.

## Current Classification

The Cassie query-engine baseline is a **Production Candidate** for the documented pre-release
support envelope. Stable capabilities are supported; Experimental capabilities are available for
evaluation under their documented limits and may change before 1.0.

Cassie is not Production-ready. Local disk-backed smoke evidence is sufficient to catch correctness and gross resource-bound regressions, but it is not representative-scale evidence for production latency, capacity, cancellation latency, recovery time, or sustained concurrency.

## Evidence Present

- Locked build, full test, pedantic Clippy, and formatting gates.
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
- Deterministic local-server contracts for OpenAI, OpenAI-compatible, TEI, Ollama, Voyage, Cohere, and local embeddings, including request shape, ordering, dimensions, retry deadlines, transport timeouts, and active cancellation.
- Remote embedding-provider evidence is mock-provider evidence: it covers authentication headers,
  HTTP 429 handling, bounded retry attempts, query-deadline clamping, response limits, and
  deterministic failures. It does not establish hosted availability, provider quotas, model
  availability, service latency, or account-specific authorization. Operators must monitor the
  selected service and size retry counts and query deadlines to its published rate-limit policy.
- Security-boundary coverage for constant-cost authentication failures, bounded login state, atomic REST session quotas, explicit external HTTPS attributes, bounded provider responses, streaming transport deadlines, parser complexity budgets, and live database-access revocation.
- Health, metrics, EXPLAIN, projection diagnostics, capacity guidance, snapshot/restore guidance, and repair runbooks.
- Container and supply-chain workflows for supported targets.
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
  vectorized-to-merge inner/left equi-join switching. Backend CI retains a same-commit
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
- Canonical v2 column-batch format and corruption tests, automatic typed codecs, selected-value dictionary and FSST decoding, encoded scan and filtered-aggregate parity, generation-fenced range copy-on-write DML, and paired Tier 2 codec acceptance gates. ALP codec version 1 has the retained `should_emit_cross_architecture_stable_alp_bytes` golden fixture, sparse-null and exact-result fallback/rebuild coverage, unknown-version fail-closed coverage, and paired `perf.column.alp_selective_scan.2k` / `perf.column.alp_plain_scan_baseline.2k` owners enforcing at least 75% chunk-byte savings and no more than 5% p95 query regression. FSST codec version 1 has the retained `should_emit_cross_architecture_stable_fsst_bytes` full-CBC2 golden fixture plus sparse-null selected decoding, corrupt-unselected-value rejection, fallback/restart rebuild, and rename/drop lifecycle coverage. Its paired `perf.column.fsst_selective_scan.2k` / `perf.column.fsst_plain_scan_baseline.2k` owners require every candidate chunk to save at least 25% and the selective-query p95 to remain within 5% of forced plain. Each of the four candidate/baseline pairs owns an independent sampling group and alternates within each invocation so unrelated pairs cannot create sustained drift or bias the comparison.
- Locked UI install, production-dependency audit, generated-client freshness, tests, type checking, lint, and production build.
- Production-browser coverage runs the Admin UI from a real temporary Cassie process at desktop and mobile viewports. The Askr `0.2.1`, Askr UI `0.2.0`, `@askrjs/themes` `0.2.1`, and `@askrjs/monaco` `0.2.0` package run passes 116 repository tests, 20 desktop/mobile mock-browser cases, and 2 real-Cassie desktop/mobile production-browser cases together. Mock-browser evidence covers axe scans of login, empty workspace, controlled dialog, populated results, query failure, and mobile-sidebar states plus committed populated desktop light/dark and mobile open/closed screenshots. The production-browser case loads a fresh Monaco editor, immediately filters the schema tree, and rejects the historical `state.set() cannot be called during component render` console failure. Cassie isolates live query-text reactivity from Monaco host reconciliation so editor selection, history, completion, results, and theme changes preserve workspace state. The released packages resolve [askrjs/askr-monaco#22](https://github.com/askrjs/askr-monaco/issues/22) and [askrjs/askr-themes#62](https://github.com/askrjs/askr-themes/issues/62), so their application workarounds are removed. This evidence promotes only the Admin UI support entry to Stable; Cassie's overall Production Candidate classification is unchanged.
- Askr does not yet expose a lifecycle-owned dynamic keyed-query collection with bounded prefetch and aggregate state. Cassie therefore retains one narrow `DatabaseCatalogController` for lazy schema loading, three-request search prefetch, retry, aggregate progress, and abort-on-unmount without adding another schema cache. The upstream capability is tracked in [askrjs/askr#327](https://github.com/askrjs/askr/issues/327).

## Production Candidate Support Envelope

- PostgreSQL wire is the primary SQL interface; REST is secondary and administrative.
- Only capabilities marked Stable are supported contracts. Experimental capabilities are evaluation surfaces, not compatibility commitments.
- Midge is the only storage layer. Cassie is permanently single-node and does not provide distributed SQL, cluster management, replication, consensus, sharding or rebalancing, cross-node transactions, distributed planning, remote query forwarding, or automatic cross-node repair.
- The Production Candidate bar requires the validation sequence in [Definition of Done](definition-of-done.md), UI production-dependency audit and gates, benchmark-owner compilation, and a disk-backed smoke run on the release commit.
- Smoke results are regression diagnostics, not service-level objectives or capacity claims.

## Remaining Production Blockers

- Database-image checksums detect content changes but do not authenticate who produced an image.
  Operators must follow the detached-signature and trusted-identity procedure in
  [Snapshot, Backup, Restore, and Repair](snapshot-restore.md) before streaming an image from an
  untrusted channel into `RESTORE`; Cassie intentionally does not define another signing format.
- Keep Monaco dependency-upgrade tracking with [microsoft/monaco-editor#5352](https://github.com/microsoft/monaco-editor/issues/5352). Cassie pins DOMPurify `3.4.13` through its package override; the production-dependency audit currently reports zero vulnerabilities.

- Retain complete same-commit Tier 1-6 artifacts for
  `workstation-apple-m5-arm64-apfs`, including the two-hour Tier 6 set, and add
  representative native-Linux capacity and soak evidence before a global Production-ready
  claim.
- Establish and validate operational thresholds for disk growth, resource admission, backup/restore time, rebuild and repair time, failure injection, cancellation latency, and sustained mixed workloads.
- Exercise container startup, health, restart, snapshot, restore, and failure-recovery runbooks in each supported release architecture and deployment profile.
- The cross-architecture rehearsal record is defined in [Cross-Architecture Release Rehearsal](cross-architecture-rehearsal.md); amd64 and arm64 evidence remains pending until separate retained bundles exist.
- The `Operational readiness` workflow separates a fast native amd64/arm64 shape gate from its
  scale and endurance mode. A passing shape run validates the automation but does not satisfy the
  pending retained-evidence blocker.
- Validate the support, upgrade compatibility, release rollback, and security response expectations
  in [Support, Release, and Security-Response Policy](support-policy.md) against a tagged release
  artifact and rehearsal evidence.

## Promotion Evidence

A Production-ready claim must link to the exact commit, toolchain, deployment profile, configuration, fixture, complete owner-suite artifacts, restart or recovery evidence, resource-bound measurements, and known limitations. Evidence must include result correctness, selected access paths, fallback reasons, storage reads, candidates, peak accounted query memory, workers, and cancellation latency. Local fallback-storage results remain developer diagnostics.

Midge evidence owns persistence, durability, and recovery mechanics. Cassie readiness evidence owns logical layout compatibility, query-visible failure behavior, adapter integration, restart hydration, and query semantics over recovered data.
