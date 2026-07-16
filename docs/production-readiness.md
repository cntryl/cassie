# Production Readiness

This document is the canonical owner for readiness evidence and remaining Production-ready blockers. Feature behavior and status live in [Feature Support](feature-support.md). Passing unit or integration tests does not by itself make a feature Production-ready.

## Current Classification

The Cassie query-engine baseline is not Production-ready. The repository has broad correctness and local benchmark coverage, but it does not yet retain deployment-profile evidence sufficient for a production latency, capacity, cancellation-latency, or sustained-concurrency claim.

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

## Remaining Blockers

- Complete the bounded pull-execution, memory-accounting, portal, and cancellation contract across every query family.
- Close execution-result-cache isolation, volatility, byte accounting, and concurrent invalidation evidence.
- Prove compact query-hot layout ordering, corruption behavior, and the required byte reduction.
- Prove time-series, graph, and column-batch access paths without hidden full-corpus rebuilds.
- Close adaptive switching and configured parallel-worker equivalence and oversubscription tests.
- Retain complete same-commit benchmark artifacts from a named disk-backed deployment profile at representative fixture sizes and concurrency.
- Establish operational thresholds and runbooks for disk growth, resource admission, backup/restore time, failure injection, and sustained mixed workloads.

## Promotion Evidence

A Production-ready claim must link to the exact commit, toolchain, deployment profile, configuration, fixture, complete owner-suite artifacts, restart or recovery evidence, resource-bound measurements, and known limitations. Evidence must include result correctness, selected access paths, fallback reasons, storage reads, candidates, peak accounted query memory, workers, and cancellation latency. Local fallback-storage results remain developer diagnostics.

Midge evidence owns persistence, durability, and recovery mechanics. Cassie readiness evidence owns logical layout compatibility, query-visible failure behavior, adapter integration, restart hydration, and query semantics over recovered data.
