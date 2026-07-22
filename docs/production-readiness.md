# Production Readiness

This document is the canonical owner for beta and Production-ready evidence. Feature behavior and status live in [Feature Support](feature-support.md). Passing unit or integration tests does not by itself establish either readiness classification.

## Current Classification

The Cassie query-engine baseline is Beta-ready for the documented pre-release support envelope. Stable capabilities are supported; Experimental capabilities are available for evaluation under their documented limits and may change before 1.0.

Cassie is not Production-ready. Local disk-backed smoke evidence is sufficient to catch correctness and gross resource-bound regressions, but it is not representative-scale evidence for production latency, capacity, cancellation latency, recovery time, or sustained concurrency.

## Evidence Present

- Locked build, full test, pedantic Clippy, and formatting gates.
- Integration coverage across SQL, indexes, transactions, pgwire, REST, search, vector, analytics, projection lifecycle, and recovery adapters.
- Restart and generation-fencing coverage for multiple persisted derived artifacts.
- Tiered benchmark owners with environment-labelled local observations.
- Persisted full-text SQL evidence covering posting reads, exact BM25 equivalence, snippets, structured prefilters, bounded candidate row fetches, transaction overlays, corruption fallback, cancellation, and memory limits.
- Exact, HNSW, IVFFlat, and hybrid evidence covering bound parameters, persisted candidates, exact reranking, structured filters, deletion visibility, explicit fallback diagnostics, cancellation, hard memory limits, and at least 0.90 ANN recall on deterministic 10k and 100k disk-backed fixtures.
- Deterministic local-server contracts for OpenAI, OpenAI-compatible, TEI, Ollama, Voyage, Cohere, and local embeddings, including request shape, ordering, dimensions, retry deadlines, transport timeouts, and active cancellation.
- Health, metrics, EXPLAIN, projection diagnostics, capacity guidance, snapshot/restore guidance, and repair runbooks.
- Container and supply-chain workflows for supported targets.
- Bounded pull execution, portal streaming, cancellation, result-cache isolation and invalidation, compact row layout, specialized access paths, and shared worker-permit coverage.
- Locked UI install, production-dependency audit, generated-client freshness, tests, type checking, lint, and production build.
- Production-browser coverage runs the Experimental Admin UI from a real temporary Cassie process at desktop and mobile viewports. This evidence does not promote the Admin UI or broaden Cassie's readiness classification.

## Beta Support Envelope

- PostgreSQL wire is the primary SQL interface; REST is secondary and administrative.
- Only capabilities marked Stable are supported contracts. Experimental capabilities are evaluation surfaces, not compatibility commitments.
- Midge is the only storage layer. Cassie is permanently single-node and does not provide distributed SQL, cluster management, replication, consensus, sharding or rebalancing, cross-node transactions, distributed planning, remote query forwarding, or automatic cross-node repair.
- The beta bar requires the validation sequence in [Definition of Done](definition-of-done.md), UI production-dependency audit and gates, benchmark-owner compilation, and a disk-backed smoke run on the release commit.
- Smoke results are regression diagnostics, not service-level objectives or capacity claims.

## Remaining Production Blockers

- Track the upstream low-severity DOMPurify advisory inherited through Monaco (`GHSA-c2j3-45gr-mqc4`); no fixed dependency version is currently available. The frontend gate continues to fail on moderate-or-higher production advisories.

- Retain complete same-commit benchmark artifacts from a named disk-backed deployment profile at representative fixture sizes and concurrency.
- Establish and validate operational thresholds for disk growth, resource admission, backup/restore time, rebuild and repair time, failure injection, cancellation latency, and sustained mixed workloads.
- Exercise container startup, health, restart, snapshot, restore, and failure-recovery runbooks in each supported release architecture and deployment profile.
- Define support policy, upgrade compatibility, release rollback, and security response expectations for the production service envelope.

## Promotion Evidence

A Production-ready claim must link to the exact commit, toolchain, deployment profile, configuration, fixture, complete owner-suite artifacts, restart or recovery evidence, resource-bound measurements, and known limitations. Evidence must include result correctness, selected access paths, fallback reasons, storage reads, candidates, peak accounted query memory, workers, and cancellation latency. Local fallback-storage results remain developer diagnostics.

Midge evidence owns persistence, durability, and recovery mechanics. Cassie readiness evidence owns logical layout compatibility, query-visible failure behavior, adapter integration, restart hydration, and query semantics over recovered data.
