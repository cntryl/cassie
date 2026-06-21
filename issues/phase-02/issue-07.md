# Phase 02 Issue 07: Projection Rebuild Performance Targets

Milestone: Read-Model Core
Area: Benchmarks
Status: Open
Priority: P1

## Requirements

Define and measure performance targets for projection replay, rebuild, verification, swap, and lag catch-up workflows before calling read-model lifecycle features production-ready.
This issue creates the benchmark evidence needed to prioritize optimization work without changing product semantics.

## Dependencies

- Depends on phase 01 projection lifecycle issues for replay, rebuild, version, and swap workflows.
- Depends on phase 02 issues 01 through 06 for hash, verification, operations, and integrity workloads where those targets are benchmarked.

## Handoff

- Provides baseline numbers and target ranges for future performance optimization issues.

## Functional Scope

- Add benchmark scenarios for replay ingestion throughput, duplicate handling, projection rebuild from row blobs, rebuild verification, version swap latency, and lag catch-up.
- Start with 10k row/event scale and add larger scale points only after the 10k path is stable.
- Document minimum/good/excellent target ranges for each measured read-model lifecycle workflow.
- Capture throughput, elapsed time, p50/p95 latency where applicable, memory-sensitive counters where available, and relevant Midge write/read counts.
- Use deterministic data generation and isolated storage paths or in-memory fallback so benchmark runs are comparable.
- Keep benchmarks tied to row blobs, Midge metadata, and existing executor/query paths.

## Non-Goals

- Do not optimize implementation paths in this issue unless needed to make the benchmark meaningful.
- Do not add external event-store dependencies.
- Do not define production SLA commitments from local benchmark numbers alone.

## Acceptance Criteria

- Benchmarks exist for the core read-model lifecycle workflows at the documented scale.
- Benchmark docs explain what each workflow measures and which production-readiness claim it supports.
- Results expose enough detail to distinguish ingestion, rebuild, verification, swap, and query-serving costs.
- Benchmarks can run independently through the tiered benchmark suite.
- Benchmark fixtures do not require services outside Cassie and Midge.

## Required Tests

- Add or update benchmark support code and run compile/build validation for benches.
- Add focused unit/integration tests only where benchmark setup requires new reusable behavior.
- Include at least one smoke path proving benchmark fixture generation is deterministic if new reusable fixture code is added.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by benchmarks/tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs with benchmark targets and commands.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo bench --locked --bench tier3_system_rebuild --no-run`
- `cargo bench --locked --bench tier2_subsystem_ingest --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
