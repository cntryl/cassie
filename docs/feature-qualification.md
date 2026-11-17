# Feature Qualification

Cassie is a pre-release project. Qualification records describe concrete
implemented behavior and the evidence for it, including unsupported cases,
known failures and work not yet executed. They do not assign a maturity tier
to a feature or change the project's release posture.

Evidence is specific to a surface, revision, configuration and environment.
Broad integration coverage or a completed milestone does not establish an
unmeasured compatibility, durability, performance or deployment claim.

## Common Evidence

A feature or qualification issue records:

- Implemented, partial, planned and unsupported behavior in [Feature Support](feature-support.md).
- Deterministic errors for reachable unsupported paths.
- Restart, hydration, rename/drop and cleanup tests when metadata persists.
- EXPLAIN, metrics, catalog or protocol diagnostics for non-obvious planner/executor behavior.
- Compatibility notes for pgwire-visible SQL, catalog and error behavior.
- Benchmark or deployment-profile evidence for performance-sensitive paths.
- Operator documentation for admin, recovery, repair, capacity and external orchestration workflows.
- A [deployment evidence](production-readiness.md) update linking actual results and remaining gaps.

Authored tests are not executed evidence. Reports distinguish passed, failed,
running, canceled and blocked checks and identify their exact source. Memory
and cancellation limits, permission checks, fallback behavior and recovery
requirements remain part of each contract.

## Surface Evidence

| Surface | Concrete scope | Evidence and limits |
| --- | --- | --- |
| Catalog metadata and virtual views | Selected named views; PostgreSQL parity is partial. | Record documented columns, deterministic ordering, restart behavior, SQLSTATEs for unsupported probes and client compatibility. Keep PostgreSQL-internal parity claims outside unimplemented scope. |
| Limited procedures and `CALL` | The narrow persisted statement contract. | Record body semantics, argument binding, restart hydration and pgwire metadata; retain deterministic rejection of PL/pgSQL, triggers, dynamic SQL, transaction control, recursion and business-procedure workflows. |
| Rollups and retention | Explicit local operator workflows. | Record refresh/enforcement idempotency, restart hydration, stale fallback, dependent-projection freshness, metrics and benchmark results at declared fixture sizes and operator windows. |
| HNSW vector indexes | Persisted graph candidates with exact source-row reranking. | Retain the native-disk recall, latency, resource, fallback and recovery contract in [HNSW Release Evidence](hnsw-release-evidence.md); keep global capacity claims separate. |
| IVFFlat vector indexes | Persisted membership-prefix candidates, deterministic probes and exact reranking. | Retain the list/probe/training/recall/resource/recovery matrix in [IVFFlat Release Evidence](ivfflat-release-evidence.md). |
| Embedding providers | Provider-specific external protocols. | Record auth/config, timeout/retry behavior, dimension/model validation, deterministic failures, hosted/self-hosted tests or mocks, rate limits and operator guidance. Local protocol evidence is not provider availability evidence. |
| Time-series indexes | Fixed positive minute/hour/day UTC index widths. | Retain range planning, mutation/delete/retention, restart, fallback and supported-width evidence. Unsupported DDL rejects; broader buckets and workload capacity require their own evidence. |
| Analytical projections and column-store tables | Cassie-specific storage and read paths. | Record DML boundaries, freshness/fallback, restart/cleanup, EXPLAIN, capacity measurements and analytical fixtures. |
| Adaptive planning and operator switching | The selected opt-in scalar-read and inner/left equi-join paths below. | Preserve disabled defaults, confidence/cost guards, stale-feedback handling, result equivalence, EXPLAIN diagnostics and rollback controls. |
| Operational metadata and capacity diagnostics | Local metadata and diagnostics. | External routing, movement, capacity admission and distributed repair remain external unless a separate product decision changes that boundary. |

## Scope Boundaries

- Procedures remain limited compatibility/admin workflows.
- Catalog evidence is specific to named client workflows and documented columns.
- EXPLAIN and metric keys follow their documented contract; additional diagnostics may be added.
- External-provider behavior remains tied to the selected provider contract and failure modes.

Future work cites this document for evidence requirements and changes only the
selected behavior. Feature qualification follows these evidence requirements under the
project's pre-release posture.

## Adaptive Evaluation Profiles

The `disabled-default` profile keeps `adaptive_execution_enabled=false` and
`operator_switching_enabled=0`. This is the default for every deployment and is the
rollback state when feedback is stale, below its confidence threshold, or fails a cost guard.

The `evaluation-conservative` profile is opt-in and uses `operator_feedback_enabled=true`,
`adaptive_execution_enabled=true`, `adaptive_min_cost_savings_bps=500`,
`adaptive_min_confidence_bps=900`, `operator_switching_enabled=1`, and
`operator_switch_join_row_threshold=4096`. It is intended for representative evidence only;
result-equivalence, deterministic EXPLAIN guard diagnostics, stale-feedback fallback, and
operator-switch rollback tests must pass before any default change is considered.

## Selected Adaptive Surface

The selected implementation boundary is narrower than the adaptive feature family:

- feedback-informed selection between a scalar row scan and an already legal scalar-index read;
- checkpointed switching from a vectorized inner/left equi-join to the existing merge-join
  implementation after the configured row threshold is crossed.

Candidate-budget expansion, new candidate generation, other join pairs, and automatic profile
selection are outside this selected scope. The selected surface is opt-in and uses the
`disabled-default` profile unless explicitly configured.

`metrics_adaptive::should_preserve_the_selected_adaptive_read_surface_across_profiles` compares the
two profiles over identical fixtures and records exact rows, ordering, pagination, transaction
visibility, errors, fallback diagnostics, selected paths, storage reads, candidate metrics, memory,
and worker state. Main-branch backend CI retains the resulting
`cassie-adaptive-profile-<commit>` artifact. The companion adaptive metrics tests cover fresh,
missing, low-confidence, below-savings, stale-TTL, stale-schema, and persistence-failure feedback,
plus join-switch success, disabled/unsupported skips, replacement failure, memory bounds, EXPLAIN,
and metrics. Rollback is exercised across restart with persisted feedback retained.

The cross-profile integration fixture uses the same explicit 64 MiB query-memory
cap for both profiles and records it in the artifact's configuration. This
fixture cap admits retained input, qualification, keyed copies and output for
the unchanged 2,050/2,047-row join. Its 4,097 input rows cross the adaptive
4,096-row replacement threshold; the fixed profile uses the existing batched
join and the adaptive profile uses merge. The owner checks actual strategies,
cardinalities, `row_threshold_exceeded` replacement reason, peak within the cap,
and released query memory and workers.
Profile names describe the selected adaptive settings; this evidence-only
working-set cap does not change deployment memory defaults.
