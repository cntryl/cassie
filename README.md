<p align="center">
  <img src="ui/public/assets/cassie-logo-256x256.png" alt="Cassie logo" width="256">
</p>

# Cassie

Cassie is a single-node read-model database for CQRS and event-sourced systems. Projectors maintain durable projections from the event stream, which remains the source of truth; applications query those projections through Cassie's PostgreSQL wire interface and documented SQL/client contract. REST is secondary and administrative.

Cassie uses `cntryl-midge` directly as its only storage layer. Midge provides persistence, durability, and recovery mechanics. Cassie provides SQL semantics, planning, execution, logical query layouts, indexes, search, analytics, resource controls, and query-visible errors.

Cassie has never been released. The package is reset to the 0.1.0 starting
baseline; Cassie-owned persistent formats start at version 1. See the
[release baseline](docs/release-baseline.md) for format and data compatibility.

## Start Here

- [Documentation map](docs/README.md)
- [Environment variables](docs/environment-variables.md)
- [Feature behavior and status](docs/feature-support.md)
- [PostgreSQL wire and client contract](docs/postgres-compatibility.md)
- [Performance contracts](docs/performance-contracts.md)
- [Production-readiness evidence](docs/production-readiness.md)
- [Product scope and roadmap](docs/product-roadmap.md)
- [0.1.0 release baseline](docs/release-baseline.md)
- [POC quickstart](docs/poc-quickstart.md)

The locked `cntryl-midge` 0.5.1 dependency uses database FORMAT 9 and SST V5.
Midge 0.4.x stores require logical export/import into a fresh store; there is
no in-place upgrade. See the [storage compatibility boundary](docs/storage-recovery-qualification.md#midge-051-fresh-store-boundary-2026-10-30).

## Local Development

```sh
cargo build --locked --bin cassie
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo fmt --all -- --check
```

Run the embedded proof of concept with:

```sh
cargo run --locked --example poc_read_model
```

For benchmark navigation, compile all owners, run a short diagnostic suite, or run the normal Tier 1-4 developer suite:

```sh
cargo bench --locked --no-run --bench '*'
STRESS_PROFILE=smoke CASSIE_BENCH_SOAK_DURATION_SECONDS=5 cargo bench --locked --bench '*'
cargo bench --locked --bench 'tier[1-4]_*'
```

The Tier 1-6 ownership, timing, fixture, evidence, and full-suite acceptance rules are canonical in [Performance Contracts](docs/performance-contracts.md).

## Container

The repository has `Publish` and `Containers` workflows configured for
`ghcr.io/cntryl/cassie`, but GitHub Actions are disabled by current user
direction. Do not enable or dispatch them until the user explicitly changes
that direction. The configured release flow is documented here for reference;
hosted publishing is currently blocked.

When authorized, the `Publish` workflow releases `ghcr.io/cntryl/cassie` from
`main`. Its called
`Containers` workflow publishes the immutable `:<semver>` manifest plus
`:main` and `:latest`, then `Publish` creates the matching annotated `v<semver>` source
tag only after the container succeeds. The standalone `Containers` workflow publishes
prerelease SemVer images for branch builds without creating a repository release tag.
See the [Release Checklist](docs/release-checklist.md).

`compose.yml` expects REST TLS to terminate at a trusted reverse proxy or load balancer. It binds the published ports to host loopback, sets `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN=1` for the private container hop, and requires a non-default `CASSIE_ROOT_PASSWORD` for the fixed `root` login. Do not publish that plaintext hop directly to an untrusted network.

The Cassie server binary also requires `CASSIE_ROOT_PASSWORD` when run directly; it does not provide a built-in root password. Set it from a secret before starting the server.

The provider-specific development profiles use the Sqrzl storage emulator. Copy
`.env.example` to `.env`, set `CASSIE_ROOT_PASSWORD`, and start one profile:

```sh
docker compose up -d                         # local disk storage
docker compose -f compose.s3.yml up -d       # S3-compatible emulator
docker compose -f compose.azure.yml up -d    # Azure Blob emulator
docker compose -f compose.gcs.yml up -d      # GCS emulator
```

The provider profiles pull `ghcr.io/sqrzl/sqrzl-emulator:latest` and connect to
it over the Compose network; the emulator is also published on host port 9000
for local diagnostics.

For Cassie to terminate REST TLS itself, remove the insecure-listener override, configure `CASSIE_REST_TLS_CERT_FILE` and `CASSIE_REST_TLS_KEY_FILE` inside the container, and mount the PEM certificate chain and private key read-only. Cassie fails closed when direct non-loopback TLS is selected but either file is absent.

When a trusted proxy terminates HTTPS, set `CASSIE_REST_EXTERNAL_HTTPS=1` so Cassie emits HSTS and marks login and logout cookies `Secure`. Cassie does not trust `Forwarded` or `X-Forwarded-Proto`. This setting does not authorize a plaintext non-loopback listener: the private proxy hop still requires `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN=1`.

## Security Resource Configuration

| Variable | Default | Contract |
| --- | ---: | --- |
| `CASSIE_AUTH_USER_ATTEMPTS_PER_MINUTE` | `10` | Process-local login token-bucket capacity per normalized user. |
| `CASSIE_AUTH_IP_ATTEMPTS_PER_MINUTE` | `60` | Process-local login token-bucket capacity per peer IP. |
| `CASSIE_AUTH_RATE_LIMIT_MAX_ENTRIES` | `4096` | Combined tracked user/IP entries; excess identities share bounded overflow buckets. |
| `CASSIE_REST_MAX_SESSIONS_PER_USER` | `16` | Active opaque REST sessions allowed for one normalized user; the global cap remains 1,024. |
| `CASSIE_REST_EXTERNAL_HTTPS` | `0` | Explicitly declares HTTPS outside Cassie for browser security attributes; forwarding headers remain ignored. |
| `CASSIE_EMBEDDINGS_MAX_RESPONSE_BYTES` | `8388608` | Maximum success or error response body accepted from a remote embedding provider. |
| `CASSIE_REST_WRITE_TIMEOUT_MS` | `10000` | REST transport write idle deadline. |
| `CASSIE_PGWIRE_WRITE_TIMEOUT_MS` | `10000` | Pgwire transport write idle deadline. |

## Product Boundaries

Cassie targets predictable, resource-bounded read-model writes, relational queries, indexes, full-text and vector retrieval, hybrid scoring, analytics, time-series access, and graph traversal. PostgreSQL wire and SQL compatibility make familiar drivers and tools usable for the documented Cassie contract; PostgreSQL 18 is a semantic comparison reference for selected behavior, not a full-server compatibility target. The [roadmap](docs/product-roadmap.md) defines product scope and release order; [feature support](docs/feature-support.md) defines current behavior.

Cassie's single-node boundary is permanent, not a pre-release omission. Cassie does not pursue distributed SQL, cluster management, membership, replication, consensus, sharding or rebalancing, cross-node transactions, multi-node query planning, remote query forwarding, OLTP optimization, trigger-based business logic, or a second storage abstraction. Deployments may run independent Cassie nodes, but external systems own routing, placement, failover, data movement, and fleet coordination.
