# Cassie

SQL-over-document-store database engine in Rust. Embedded storage via `cntryl-midge` (key-value with column families).

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
| `tests/` | Integration tests (one file per subsystem) |
| `benches/` | Tiered benchmarks: tier1 (micro), tier2 (subsystem), tier3 (system), tier4 (integration) |

## File Organization

Small, well-organized files are a core architecture requirement. Future feature work must keep modules focused so changes stay surgical.

- Put new code in the smallest domain-specific module that fits the behavior.
- Keep source and test files under 1,000 lines. If a legacy file is already over that limit, feature work in that area must extract a focused module or test file before adding behavior.
- Do not add substantial feature work to files over 1,000 lines unless the same change extracts code out of that file.
- Keep tests grouped by subsystem. Do not add new broad coverage to catch-all integration files when a subsystem-specific test file exists.
- Prefer refactors that reduce oversized files before adding more behavior to them.
- Use this audit when planning large work:

```sh
find src tests benches -type f -name '*.rs' -print0 | xargs -0 wc -l | sort -nr | head -40
```

## Testing

- All integration tests live in `tests/`. Module tests live near the code they cover.
- **Async tests**: use `tokio::runtime::Builder::new_current_thread().enable_all().build()` — never `#[tokio::test]`.
- Many tests need `CASSIE_MIDGE_ALLOW_FALLBACK=1` for in-memory storage fallback.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` is a standing requirement for completed work.
- Fix every pedantic clippy finding in touched code. Do not silence clippy with `#[allow(clippy::...)]`, lint-cap changes, reduced lint levels, or equivalent workarounds.

```sh
# specific split integration tests
cargo test --locked --test integration_sql_query -- --nocapture
cargo test --locked --test executor_parallel -- --nocapture

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
| `CASSIE_REST_TLS_CERT_FILE` | — | PEM certificate chain; required with the key for non-loopback REST listeners |
| `CASSIE_REST_TLS_KEY_FILE` | — | PEM private key; required with the certificate for non-loopback REST listeners |
| `CASSIE_MIDGE_DATA_DIR` | `./.cassie/midge` | |
| `CASSIE_MIDGE_ALLOW_FALLBACK` | — | Set `1` for tests (in-memory) |
| `CASSIE_ADMIN_PASSWORD_FILE` | — | Path to read password from (takes precedence over `CASSIE_ADMIN_PASSWORD`) |
| `CASSIE_EMBEDDINGS_PROVIDER` | `disabled` | `openai`, `ollama`, `tei`, `openai_compatible`, `voyage`, `cohere`, `local` |
| `CASSIE_QUERY_TIMEOUT_MS` | `30000` | `0` = no deadline |

Password can come from `CASSIE_ADMIN_PASSWORD` or `CASSIE_ADMIN_PASSWORD_FILE` (file path, read at startup).

## Boundaries

- **Midge** is the only storage layer. No second abstraction.
- **PostgreSQL wire protocol** is the primary query interface. REST is secondary and administrative.
- `cntryl-midge` is an external dependency via git: `https://github.com/cntryl/midge`.

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

Multi-arch image published to `ghcr.io/cntryl/cassie`. Built via GitHub Actions (`containers.yml`):
`linux/amd64` and `linux/arm64`, tagged with GitVersion semver and branch name.

## Versioning

GitVersion `ContinuousDeployment` mode, next version `0.2.0`. Branch naming conventions in `GitVersion.yml`.

## Constraints

- Keep Midge as the direct storage layer for V1.
- Do not introduce a second storage abstraction.
- Keep PostgreSQL wire protocol as the primary query interface.
- Keep REST secondary and administrative.

## Agentic Workflow

Agents must work from the current repository source of truth, not from ad hoc architectural judgment.
When an `issues/` backlog exists, follow its priority order. In this checkout, no issue backlog is present; use the user request plus `docs/product-roadmap.md`, `docs/production-readiness.md`, `docs/performance-contracts.md`, and subsystem docs as the planning surface.

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
