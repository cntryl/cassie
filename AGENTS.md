# Cassie

SQL-over-document-store database engine in Rust. Embedded storage via `cntryl-midge` (key-value with column families).

## Quick Start

```sh
cargo build --locked --bin cassie
cargo test --locked
cargo bench --locked
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

```sh
# specific split integration tests
cargo test --locked --test integration_sql_query -- --nocapture
cargo test --locked --test executor_parallel -- --nocapture

# single unit test
cargo test --locked should_reuse_cached_plan_arc

# validate test files with cntryl-tools
cntryl-tools validate-tests -f <path>
```

**Required order**: `cargo build` → `cargo test` → `cntryl-tools validate-tests -f <path>`

## Configuration

All config via `CASSIE_*` env vars (see `src/config.rs`):

| Variable | Default | Notes |
|----------|---------|-------|
| `CASSIE_PGWIRE_LISTEN` | `127.0.0.1:5432` | |
| `CASSIE_REST_LISTEN` | `127.0.0.1:8080` | |
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

## Workflow

1. Write a failing test (TDD, `should_` prefix, `// Arrange / Act / Assert`).
2. Make smallest code change to pass.
3. Refactor without broadening test scope.
4. `cargo build && cargo test --locked && cargo fmt --all -- --check`
5. `cntryl-tools validate-tests -f <path>` on touched test files.
