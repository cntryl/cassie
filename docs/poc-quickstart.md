# POC Quickstart

Use this path when the goal is a local proof of concept, not production readiness.
It runs Cassie embedded with Midge's in-memory fallback, creates a small read-model table, writes
projection rows, adds a scalar index, runs SQL queries, prints EXPLAIN output, and reports metrics.

```sh
cargo run --locked --example poc_read_model
```

Expected output includes:

```text
Cassie POC read model
health.ready=true
open_orders=
tenant_totals=
plan=
queries_recorded=
```

This POC does not start pgwire or REST, does not use disk-backed Midge unless you adapt the example,
and does not imply production capacity thresholds. Use it to prove the embedded read-model loop
before moving to container, pgwire, benchmark, or deployment-profile work.
