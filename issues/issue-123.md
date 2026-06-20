# Issue 123: Operator Selection Feedback

Milestone: V4 - Analytical Overlay
Area: Adaptive Planning
Status: Open
Priority: P3

## Requirement

Feed observed operator performance back into future operator selection without compromising deterministic planning or correctness.

## Functional Scope

- Aggregate runtime feedback by normalized operator shape, collection, schema epoch, and relevant predicate/index characteristics.
- Compare estimated versus actual rows, elapsed time, storage reads, temp writes, and memory/spill indicators.
- Adjust future cost inputs for eligible operator alternatives when feedback is fresh and statistically meaningful.
- Bound feedback influence so a single outlier cannot permanently bias planning.
- Expose feedback use, age, confidence, and ignored/outlier status through EXPLAIN/metrics.

## Non-Goals

- Do not switch operators during an already-running query; that is issue 140.
- Do not make planning depend on bind values that are not part of the normalized safe key.

## Acceptance Criteria

- Repeated workloads can influence future operator selection when feedback is consistent.
- Feedback is invalidated or ignored across schema/catalog changes and stale epochs.
- Missing, low-confidence, or outlier feedback falls back to base cost estimates.
- Query results remain identical regardless of feedback availability.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback aggregation, plan influence, stale feedback invalidation, outlier damping, missing feedback fallback, and EXPLAIN diagnostics.
- Include planner and metrics tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document feedback confidence/retention policy.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
