# Phase 07 Issue 01: Operator Selection Feedback

Milestone: Advanced Backlog
Area: Planner Intelligence
Status: Open
Priority: P3

## Requirements

Feed observed operator performance back into future operator selection without compromising deterministic planning or correctness.
This issue extends earlier cost and index feedback work from access-path choice into broader physical operator choice.

## Dependencies

- Depends on phase 03 issue 02 for cost-informed planning.
- Depends on phase 03 issue 03 for safe runtime feedback collection and feedback invalidation patterns.
- Depends on phase 03 issue 10 for advanced cardinality estimates used as the base planner signal.
- Depends on phase 02 issue 05 for bounded metrics and EXPLAIN diagnostics.
- Depends on the archived phase 04 runtime-boundary contract surface in `docs/performance-contracts.md` and `issues/phase-04/README.md` for runtime-boundary metrics vocabulary.
- Depends on phase 06 issue 05 for access-path and executor diagnostic assertions.

## Handoff

- Provides confidence-scored operator feedback consumed by phase 07 issue 05 adaptive execution plans.
- Provides operator-level telemetry that phase 07 issue 06 runtime operator switching can use for threshold selection, without enabling switching itself.

## Functional Scope

- Aggregate runtime feedback by normalized operator shape, collection, schema epoch, and relevant predicate/index characteristics.
- Compare estimated versus actual rows, elapsed time, storage reads, temp writes, and memory/spill indicators.
- Adjust future cost inputs for eligible operator alternatives when feedback is fresh and statistically meaningful.
- Bound feedback influence so a single outlier cannot permanently bias planning.
- Persist feedback through Midge/catalog metadata with bounded retention, bounded key cardinality, and explicit schema/catalog epoch invalidation.
- Keep feedback as a cost input only; it must not alter SQL semantics, authorization, freshness checks, or available operator eligibility.
- Provide controls to disable feedback use and to inspect the base estimate versus feedback-adjusted estimate.
- Expose feedback use, age, confidence, and ignored/outlier status through EXPLAIN/metrics.
- Use only bounded labels already allowed by phase 04 and phase 06 diagnostics; do not add SQL text, bind values, credentials, or row payloads to feedback keys.

## Non-Goals

- Do not switch operators during an already-running query; that is phase 07 issue 06.
- Do not make planning depend on bind values that are not part of the normalized safe key.
- Do not use feedback to select an operator that failed semantic eligibility checks.

## Implementation Plan

### Step 1: Define stable feedback contract

- Add a normalized key format for operator-shape feedback ownership:
  - operator family
  - collection or source relation set
  - schema/catalog epoch
  - predicate/index shape hash
- Add bounded cardinality limits and invalidation points for stale schemas and dropped relations.

### Step 2: Add feedback collection points

- Capture runtime deltas for each completed physical operator execution:
  - rows produced/read
  - elapsed time
  - retries and errors
  - read/write counters
  - memory and spill indicators
- Emit a bounded sample record so a single outlier cannot contaminate all future plans.

### Step 3: Build an explicit feedback aggregator

- Aggregate sampled observations into decay/quantile style compact state.
- Track confidence, age, volume, and ignore flags.
- Keep confidence below a hard floor unless sample count and consistency checks pass.

### Step 4: Consume feedback in planner

- Add a cost-modifier path that can apply feedback deltas only for eligible pre-validated operator choices.
- Ensure the planner can always fall back to base cost when feedback is missing, stale, or disabled.
- Preserve all original operator eligibility checks.

### Step 5: Add diagnostic visibility

- Add plan-level labels for feedback-used/ignored states.
- Add metrics for ineligible feedback and outlier rejection.
- Add session/global controls to disable feedback by config.
- Expose base versus adjusted estimate in `EXPLAIN`.

### Step 6: Validation and close-out

- Add `should_` tests for aggregation, confidence gating, stale/invalidation behavior, outlier damping, disabled mode, and deterministic fallback.
- Verify planner behavior with representative test fixtures for safe shape reuse.
- Add test/benchmark assertions for stable plan selection under stable feedback.

## Acceptance Criteria

- Repeated workloads can influence future operator selection when feedback is consistent.
- Feedback is invalidated or ignored across schema/catalog changes and stale epochs.
- Missing, low-confidence, or outlier feedback falls back to base cost estimates.
- Query results remain identical regardless of feedback availability.
- Disabling feedback produces the deterministic base plan and is visible in diagnostics.
- Feedback keys and metric labels remain bounded under varied query text and bind values.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback aggregation, plan influence, stale feedback invalidation, outlier damping, disabled mode, bounded key generation, missing feedback fallback, and EXPLAIN diagnostics.
- Include planner and metrics tests.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test planner_estimates --test planner_indexes --test planner_physical`
- `cargo test --locked --test metrics_feedback --test metrics_runtime --test metrics_search --test metrics_plan_pgwire`
- `cargo test --locked --test plan_cache --test integration_sql_ordering --test integration_sql_projection`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
