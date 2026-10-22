# Cassie

Single-node read-model database and query engine for CQRS and event-sourced systems. Projectors maintain durable projections from an external event stream; applications use the PostgreSQL wire interface and documented Cassie SQL/client contract. Embedded storage uses `cntryl-midge` directly as the only storage layer. Cassie is not a full PostgreSQL server or a distributed database. See `docs/product-roadmap.md` for canonical product scope and release order.

## Quick Start

```sh
cargo build --locked --bin cassie
cargo test --locked
cargo bench --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo fmt --all -- --check
```

## Architecture

| Directory | Purpose |
|-----------|---------|
| `src/sql/` | Parser, AST, binder |
| `src/planner/` | Logical plan, optimizer, physical plan |
| `src/executor/` | Scan, filter, projection, sort, aggregate, batch exec |
| `src/midge/` | Storage adapter wrapping `cntryl-midge` |
| `src/catalog/` | Schema, roles, indexes, constraints, views, functions |
| `src/pgwire/` | PostgreSQL wire protocol server (port 5432) |
| `src/rest/` | HTTP REST API (port 8080) |
| `src/embeddings/` | Providers: openai, ollama, tei, voyage, cohere, local |
| `src/vector/` | Distance metrics (cosine, dot, L2) + HNSW, IVFFlat |
| `src/search/` | BM25 full-text search, inverted index |
| `src/types/` | Value types, schema, row encoding |
| `src/hybrid/` | Hybrid text+vector scoring |
| `tests/` | 18 flat integration suites plus shared fixtures in `tests/support/` |
| `benches/` | Tiered benchmarks: tier1 (micro), tier2 (subsystem), tier3 (system), tier4 (integration) |

## File Organization

Small, well-organized files are a core architecture requirement. Future feature work must keep modules focused so changes stay surgical.

- Put new code in the smallest domain-specific module that fits the behavior.
- Keep source, benchmark, and test-support files under 1,000 lines. Flat integration suite files under `tests/*.rs` may grow to 12,000 lines when their tests remain separated into focused named modules.
- Do not add substantial feature work to files over their limit unless the same change extracts focused code or fixtures from that file.
- Keep tests grouped by subsystem. Do not add new broad coverage to catch-all integration files when a subsystem-specific test file exists.
- Put reusable fixtures, builders, mock servers, environment guards, failpoint locks, and shared assertions in focused `tests/support/*.rs` modules.
- Prefer refactors that reduce oversized files before adding more behavior to them.
- Use this audit when planning large work:

```sh
find src tests benches -type f -name '*.rs' -print0 | xargs -0 wc -l | sort -nr | head -40
```

## Testing

- Correctness tests use the smallest fixture that proves the invariant. Throughput,
  scaling, saturation, and endurance workloads belong in `benches/`, using the
  existing stress tiers. Do not add performance timing assertions to correctness tests.
- Resource tests lower configured limits and exercise just-below/just-above cases.
  Cancellation and concurrency tests synchronize at the relevant execution boundary;
  do not seed large inputs or sleep to win a scheduling race. Deadlines remain hang guards.
- Start network fixtures with an owned, bound listener instead of a fixed startup sleep.
  Use `Cassie::new_in_memory_with_config` for ephemeral SQL/protocol fixtures;
  do not mutate process-wide storage environment variables to select their mode.
  Keep restart/persistence tests on local storage and retain their actual recovery assertions.
- Use Midge memory mode for every Cassie test fixture unless the test exercises disk-backed
  persistence, restart/recovery, or on-disk layout. Keep those cases explicitly local with
  isolated data directories.
- All integration tests live in `tests/`. Module tests live near the code they cover.
- **Async tests**: use `tokio::runtime::Builder::new_current_thread().enable_all().build()` — never `#[tokio::test]`.
- Benchmark fixtures select `CASSIE_STORAGE_MODE=memory` for ephemeral runs and
  `local` explicitly when restart or persisted-state behavior is under test.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` is a standing requirement for completed work.
- Fix every pedantic clippy finding in touched code. Do not silence clippy with `#[allow(clippy::...)]`, lint-cap changes, reduced lint levels, or equivalent workarounds.

```sh
# specific split integration tests
cargo test --locked --test sql_queries integration_sql_joins:: -- --nocapture
cargo test --locked --test executor executor_parallel:: -- --nocapture

# single unit test
cargo test --locked should_reuse_cached_plan_arc

# validate test files with cntryl-tools
cntryl-tools validate-tests -f <path>
```

**Required order**: `cargo build` → `cargo test --locked` → `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` → `cargo fmt --all -- --check` → `cntryl-tools validate-tests -f <path>`

## Configuration

All config via `CASSIE_*` env vars (see `src/config.rs`):

| Variable | Default | Notes |
|----------|---------|-------|
| `CASSIE_PGWIRE_LISTEN` | `127.0.0.1:5432` | |
| `CASSIE_REST_LISTEN` | `127.0.0.1:8080` | |
| `CASSIE_REST_TLS_CERT_FILE` | — | PEM certificate chain; pair with the key when Cassie terminates REST TLS |
| `CASSIE_REST_TLS_KEY_FILE` | — | PEM private key; pair with the certificate when Cassie terminates REST TLS |
| `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN` | `0` | Set `1` only for a trusted private hop behind a TLS-terminating reverse proxy or load balancer |
| `CASSIE_STORAGE_MODE` | `local` | `memory`, `local`, or `cloud` |
| `CASSIE_STORAGE_PATH` | `./.cassie` | Persistent path used in local mode |
| `CASSIE_STORAGE_PROVIDER` | — | Cloud provider; valid values are documented in `docs/environment-variables.md` |
| `CASSIE_ROOT_PASSWORD` | `postgres` | Password for the fixed `root` administrative identity |
| `CASSIE_EMBEDDINGS_PROVIDER` | `disabled` | `openai`, `ollama`, `tei`, `openai_compatible`, `voyage`, `cohere`, `local` |
| `CASSIE_QUERY_TIMEOUT_MS` | `30000` | `0` = no deadline |

The administrative username is always `root`; set its password with `CASSIE_ROOT_PASSWORD`.

## Boundaries

- **Midge** is the only storage layer. No second abstraction.
- **PostgreSQL wire protocol** is the primary query interface. REST is secondary and administrative.
- `cntryl-midge` is an external dependency published on crates.io.

## Benchmarks

Tiered benchmark suite using criterion. Run specific tiers:

```sh
cargo bench --locked --bench tier1_hotpath_row_codec
cargo bench --locked --bench tier2_subsystem_sql_planning
cargo bench --locked --bench tier3_system_query
cargo bench --locked --bench tier4_integration_pgwire
```

Tier config overridable via `BENCH_TIER*` env vars (see `benches/support/criterion_config.rs`).

## Containers

Multi-arch image published to `ghcr.io/cntryl/cassie`. The publishing workflow
is configured in `.github/workflows/containers.yml`, but GitHub Actions are
disabled by user direction; do not enable or dispatch it until the user
explicitly changes that direction. When authorized, it builds:
`linux/amd64` and `linux/arm64`, tagged with the Cargo.toml SemVer (`main` builds use the stable
version; branch builds add a `-<branch>.<run>` prerelease suffix).

## Versioning

The version is `[package].version` in `Cargo.toml` (currently `0.2.0`). `.github/workflows/version.yml`
reads it and derives the branch prerelease suffix.

## Constraints

- Keep Midge as the direct storage layer for V1.
- Do not introduce a second storage abstraction.
- Keep PostgreSQL wire protocol as the primary query interface.
- Keep REST secondary and administrative.

## Issue and PR Formatting

Create GitHub issues and pull requests as durable engineering records. Use real Markdown newlines; never submit literal `\\n` escape sequences in a body.

### Issues

- Use an explicit title prefix: `Bug:`, `Feature request:`, `Enhancement:`, or `Documentation/readiness:`.
- Apply only existing repository labels: `bug` for broken behavior, `enhancement` for feature requests and improvements, `documentation` for documentation/readiness drift, and `help wanted` only when outside contribution is independently suitable.
- Include these sections: `Type`, `Affected subsystem`, `User/operator impact`, `Current behavior`, `Expected behavior/outcome`, `Deterministic evidence`, `Confidence and support status`, and `Acceptance criteria`.
- Include exact reproduction steps or source-backed evidence with file/test/workflow/benchmark/browser route, commit or baseline, viewport/environment where applicable, and related issue or documentation links.
- For planned capabilities, state explicitly that the capability is absent and not a regression. For blockers, name the dependency, link it, record the next action, and do not claim completion.
- Link individual findings to the audit or tracking issue. Check for duplicate titles/bodies before creating a new issue.

### Pull requests

- Match the linked issue category in the title and use sentence case after the prefix, for example `Enhancement: Add ...` or `Documentation/readiness: Define ...`.
- Apply the matching existing label and link the issue with `Closes #N`, `Fixes #N`, or `Resolves #N` when the PR completes it.
- Include these sections: `Type`, `Linked issue`, `Summary`, `User/operator impact`, `Current and expected behavior`, `Scope`, `Evidence and support status`, `Validation`, `Acceptance criteria`, and `Status`.
- Record red/green TDD evidence where code changes are made. List the exact validation commands and distinguish passed, running, cancelled, and blocked checks; never report a pending check as passed.
- State whether the PR changes runtime behavior, documentation/readiness only, or operational process. Record blockers and the next action when a PR cannot proceed.
- Before merge, verify title, label, issue link, body formatting, acceptance criteria, available checks, and review status. Do not enable or dispatch workflows to manufacture hosted evidence; while Actions is disabled, record hosted checks as blocked and do not claim them passed. Use squash merge for completed slices when merge is requested.

## Agentic Workflow

Agents must work from the current repository source of truth, not from ad hoc architectural judgment.
Use `gh` CLI to read the current GitHub backlog, issue dependencies, PRs, reviews, checks, and repository settings. Follow documented priority and dependency order, using the user request plus `docs/product-roadmap.md`, `docs/production-readiness.md`, `docs/performance-contracts.md`, and subsystem docs. Use Jev at semantic decision gates and for focused review of substantial diffs; skip it when code or tests determine the answer. Treat its judgments as leads and verify them against source, executable evidence, and current GitHub state.

GitHub Actions are disabled in repository settings by user direction. Do not enable, dispatch, or otherwise start workflows until the user explicitly changes that direction. Continue with authorized local work; report hosted workflow evidence as unavailable or blocked, never as passed.

Do not create, edit, rename, or delete CI, release, or versioning configuration (`.github/workflows/**`, `GitVersion.yml`, and similar) unless the user explicitly asks for that change in the current request. If a fix seems to need one, say so and stop instead of changing it.

### Jev

Jev is TypeSafe's System One model. It returns typed judgments and probabilities; it does not store GitHub issues or prove correctness. Read issue/status data with `gh`, then submit that evidence to Jev for triage, grouping, dependency assessment, implementation review, and adversarial review. Confirm its judgments with source, failing/passing tests, and current GitHub state.

- Live API documentation: `https://docs.typesafe.ai/api.md` and `https://docs.typesafe.ai/primitives/choice.md`.
- Endpoint: `POST https://api.typesafe.ai/v1/systemone`, with `Authorization: Bearer <key>` and `Content-Type: application/json`.
- Model: `jev-latest`. Credentials are in `~/.config/typesafe/jev.key`; read them privately and never print, log, commit, or include them in request artifacts.
- Request shape: `{"model":"jev-latest","state":{...},"questions":{"question_id":{"type":"choice","instructions":"A precise judgment about the supplied evidence","criteria":{"option":"Meaning of this option","none":"No supported option"}}}}`.
- Use `choice` for competing candidates, `noul` for a yes/no probability, and `score` for an ordered rubric. A `noul` question does not need `criteria`.
- Include exact issue requirements, relevant functions and callers, sibling paths, concrete candidate SQL inputs, and test output. Ask a specific question whose answer changes the next action; avoid generic requests such as “any regression?”.
- Submit independent questions together. Keep request/response evidence under the isolated worktree's ignored `target/` directory.
- An adversarial gap probability at or above 0.30 requires a focused probe. Fix confirmed findings or link the appropriate follow-up issue. Record Jev's question, result, and the action taken in the PR review record.

### Bundling and local validation

Work one PR at a time. Bundle a substantial set of related issues when they share implementation context or observable behavior, and define the bundle before coding. Implement each behavior as a small local red → green → refactor increment. Run the complete validation once the coherent bundle is ready, then open the PR, perform adversarial review, refine, verify the final PR head, and squash merge. Read back the merged commit and issue closures before selecting the next bundle.

Use local tests and analyzers for iteration; avoid repeatedly pushing tiny changes just to wait for GitHub workflows. If an issue is already fixed, verify the acceptance criteria locally and close it with evidence instead of opening a standalone PR that changes no runtime behavior. Track test-latency findings through the backlog and preserve required checks. GitHub Actions remain disabled under the current user direction; do not re-enable or dispatch them without a new explicit instruction.

Required loop:

1. Identify the highest-priority requested or documented slice from the available source of truth.
2. Confirm every dependency named by the request, docs, or touched subsystem is complete or already implemented.
3. Follow the requested or documented implementation plan exactly unless repo reality makes a step impossible.
4. Write the failing test first using `should_` names and `// Arrange / Act / Assert`.
5. Make the smallest code change that satisfies the failing test and issue requirements.
6. Refactor only inside the slice scope and without broadening test coverage opportunistically.
7. Update docs, diagnostics, benchmarks, or roadmap references required by the slice.
8. Run validation in the required order: `cargo build` -> `cargo test --locked` -> `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` -> `cargo fmt --all -- --check` -> `cntryl-tools validate-tests -f <path>` for touched test files.
9. Treat pedantic clippy findings as defects to fix, not lints to mute. Never add `#[allow(clippy::...)]`, never lower lint levels, and never bypass the requirement by narrowing the clippy invocation.
10. Confirm every acceptance criterion and close-out step is complete.
11. Commit the completed slice with only the files required for that slice when a commit is requested.

Do not start implementation from a later roadmap/readiness item while an earlier documented dependency is still open unless the later item explicitly names that dependency as complete or unnecessary.
Do not reinterpret documented requirements as suggestions; if the plan is wrong, update the doc or issue first so implementation remains mechanical.
Stop and ask for direction when a slice requires a persistent format decision, storage-layout migration, public API change, or cross-phase dependency that is not already specified.
