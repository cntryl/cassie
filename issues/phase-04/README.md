# Phase 04: Foundation Contracts

Phase 04 is closed for the current V1 scope.

The archived reference surface for this phase lives in `docs/performance-contracts.md`.
Use that document as the source of truth for:

- explicit async-to-sync runtime boundaries
- pgwire, REST, auth, and embedding blocking discipline
- runtime-boundary diagnostics
- read access-path vocabulary that later planner and executor work must consume without redefining

Phase 04 is not an implementation backlog anymore.
Future work should extend the archived contract only when a later issue requires a new explicit boundary or access-path concept.
