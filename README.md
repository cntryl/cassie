# Cassie

Cassie is a single-node query engine for read models in CQRS and event-sourced systems. The event stream is the source of truth; Cassie materializes and serves projection data through PostgreSQL wire protocol, with a secondary administrative REST API.

Cassie uses `cntryl-midge` directly as its only storage layer. Midge provides persistence, durability, and recovery mechanics. Cassie provides SQL semantics, planning, execution, logical query layouts, indexes, search, analytics, resource controls, and query-visible errors.

## Start Here

- [Documentation map](docs/README.md)
- [Feature behavior and status](docs/feature-support.md)
- [PostgreSQL wire and client contract](docs/postgres-compatibility.md)
- [Performance contracts](docs/performance-contracts.md)
- [Production-readiness evidence](docs/production-readiness.md)
- [Future roadmap](docs/product-roadmap.md)
- [POC quickstart](docs/poc-quickstart.md)

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

Non-loopback REST listeners require TLS. Before using `compose.yml`, set a non-default `CASSIE_ADMIN_PASSWORD`, `CASSIE_REST_TLS_CERT_HOST_FILE` to a host PEM certificate-chain path, and `CASSIE_REST_TLS_KEY_HOST_FILE` to its host PEM private-key path. The compose definition mounts those files read-only; Cassie fails closed when either is absent.

## Product Boundaries

Cassie targets predictable read-model queries: relational reads, indexes, full-text and vector retrieval, hybrid scoring, analytical projections, time-series access, and graph traversal. PostgreSQL compatibility exists to support familiar clients and SQL workflows; it is not a promise of full PostgreSQL parity.

Cassie's single-node boundary is permanent, not a pre-release omission. Cassie does not pursue distributed SQL, cluster management, membership, replication, consensus, sharding or rebalancing, cross-node transactions, multi-node query planning, remote query forwarding, OLTP optimization, trigger-based business logic, or a second storage abstraction. Deployments may run independent Cassie nodes, but external systems own routing, placement, failover, data movement, and fleet coordination.
