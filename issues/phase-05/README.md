# Phase 05: Write Optimization

Phase 05 is closed for the current V1 scope.

The archived reference surface for this phase lives in `docs/performance-contracts.md`.
Use that document as the source of truth for:

- replay and ingest batching contracts
- duplicate replay skip guarantees
- index maintenance batching and coalescing rules
- write-locality key/layout contracts
- rebuild and bulk-ingest fast paths
- write amplification diagnostics and budgets

Phase 05 is not an active backlog.
Future write-path changes must stay aligned with the archived contract surface instead of introducing a second storage abstraction or weakening deterministic replay semantics.
