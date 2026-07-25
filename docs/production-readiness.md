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
- Bounded pull execution, portal streaming, cancellation, result-cache isolation and invalidation, compact row layout, specialized access paths, and shared worker-permit coverage.
- Canonical v2 column-batch format and corruption tests, automatic typed codecs, selected-value dictionary decoding, encoded scan and filtered-aggregate parity, generation-fenced range copy-on-write DML, and paired Tier 2 codec acceptance gates.
- Locked UI install, production-dependency audit, generated-client freshness, tests, type checking, lint, and production build.
- Production-browser coverage runs the Experimental Admin UI from a real temporary Cassie process at desktop and mobile viewports. This evidence does not promote the Admin UI or broaden Cassie's readiness classification.

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
- Keep Monaco ownership and dependency-upgrade tracking with [microsoft/monaco-editor#5352](https://github.com/microsoft/monaco-editor/issues/5352). Cassie pins the patched DOMPurify release through its package override and continues to fail the frontend gate on moderate-or-higher production advisories.

- Retain complete same-commit Tier 1-6 artifacts for
  `workstation-apple-m5-arm64-apfs`, including the two-hour Tier 6 set, and add
  representative native-Linux capacity and soak evidence before a global Production-ready
  claim.
- Establish and validate operational thresholds for disk growth, resource admission, backup/restore time, rebuild and repair time, failure injection, cancellation latency, and sustained mixed workloads.
- Exercise container startup, health, restart, snapshot, restore, and failure-recovery runbooks in each supported release architecture and deployment profile.
- Define support policy, upgrade compatibility, release rollback, and security response expectations for the production service envelope.

## Promotion Evidence

A Production-ready claim must link to the exact commit, toolchain, deployment profile, configuration, fixture, complete owner-suite artifacts, restart or recovery evidence, resource-bound measurements, and known limitations. Evidence must include result correctness, selected access paths, fallback reasons, storage reads, candidates, peak accounted query memory, workers, and cancellation latency. Local fallback-storage results remain developer diagnostics.

Midge evidence owns persistence, durability, and recovery mechanics. Cassie readiness evidence owns logical layout compatibility, query-visible failure behavior, adapter integration, restart hydration, and query semantics over recovered data.
