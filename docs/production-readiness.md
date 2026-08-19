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
- Restart and generation-fencing coverage for multiple persisted derived artifacts.
- Tiered benchmark owners with environment-labelled local observations.
- Persisted full-text SQL evidence covering posting reads, exact BM25 equivalence, snippets, structured prefilters, bounded candidate row fetches, transaction overlays, corruption fallback, cancellation, and memory limits.
- Exact, HNSW, IVFFlat, and hybrid evidence covering bound parameters, persisted candidates, exact reranking, structured filters, deletion visibility, explicit fallback diagnostics, cancellation, hard memory limits, and at least 0.90 ANN recall on deterministic 10k and 100k disk-backed fixtures.
- Deterministic local-server contracts for OpenAI, OpenAI-compatible, TEI, Ollama, Voyage, Cohere, and local embeddings, including request shape, ordering, dimensions, retry deadlines, transport timeouts, and active cancellation.
- Security-boundary coverage for constant-cost authentication failures, bounded login state, atomic REST session quotas, explicit external HTTPS attributes, bounded provider responses, streaming transport deadlines, parser complexity budgets, and live database-access revocation.
- Health, metrics, EXPLAIN, projection diagnostics, capacity guidance, snapshot/restore guidance, and repair runbooks.
- Container and supply-chain workflows for supported targets.
- Named evidence boundaries are defined in [Deployment Profiles](deployment-profiles.md); the
  native-Linux profile remains an explicit evidence requirement rather than an implied claim.
- The release support, upgrade, rollback, and security-response envelope is defined in
  [Support, Release, and Security-Response Policy](support-policy.md).
- Bounded pull execution, portal streaming, cancellation, result-cache isolation and invalidation, compact row layout, specialized access paths, and shared worker-permit coverage.
- Canonical v2 column-batch format and corruption tests, automatic typed codecs, selected-value dictionary decoding, encoded scan and filtered-aggregate parity, generation-fenced range copy-on-write DML, and paired Tier 2 codec acceptance gates.
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
- Validate the support, upgrade compatibility, release rollback, and security response expectations
  in [Support, Release, and Security-Response Policy](support-policy.md) against a tagged release
  artifact and rehearsal evidence.

## Promotion Evidence

A Production-ready claim must link to the exact commit, toolchain, deployment profile, configuration, fixture, complete owner-suite artifacts, restart or recovery evidence, resource-bound measurements, and known limitations. Evidence must include result correctness, selected access paths, fallback reasons, storage reads, candidates, peak accounted query memory, workers, and cancellation latency. Local fallback-storage results remain developer diagnostics.

Midge evidence owns persistence, durability, and recovery mechanics. Cassie readiness evidence owns logical layout compatibility, query-visible failure behavior, adapter integration, restart hydration, and query semantics over recovered data.
