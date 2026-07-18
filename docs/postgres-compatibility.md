# PostgreSQL Compatibility

This document is the canonical contract for PostgreSQL wire protocol and client interoperability. SQL feature behavior and status live in [Feature Support](feature-support.md).

## Compatibility Goal

Cassie provides a PostgreSQL-like query interface for read-model workloads. It aims to work with common drivers and administration tools for documented workflows, without claiming PostgreSQL server, extension, catalog, transaction-isolation, or DDL parity.

## Connection and Authentication

- Pgwire is the primary query interface and defaults to `127.0.0.1:5432`.
- Startup negotiates protocol version, user, database, and supported parameters.
- Password authentication uses Cassie roles and stored password hashes.
- Each authenticated connection is bound to one existing database.
- Passwordless bootstrap is limited to embedded use without a network listener. Pgwire and REST listener startup reject an empty bootstrap password or a persisted passwordless bootstrap role.
- The default bootstrap password is loopback-only. Non-loopback pgwire and REST listeners require a non-default credential plus Cassie-managed TLS unless `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN=1` explicitly permits a trusted private hop behind a TLS-terminating reverse proxy or load balancer. Plaintext listener traffic must not be exposed directly to an untrusted network.
- Connection admission is bounded and failures are reported using PostgreSQL-style error responses.

## Session Model

- `current_user`, `current_database()`, `current_schema()`, `SHOW search_path`, and `SET search_path` reflect session state.
- Unqualified relations resolve through `search_path` inside the current database.
- Cross-database relation references are unsupported.
- Prepared statements and portals belong to one connection and are removed when closed or disconnected.
- Transactions accept Cassie's documented isolation behavior only; unsupported modes return `0A000`.

## Protocol Coverage

| Protocol surface | Contract |
| --- | --- |
| Simple query | One or more supported SQL statements with row descriptions, data rows, command completion, and ready state |
| Extended query | Parse, bind, describe, execute, close, flush, and sync |
| Parameters | Text and supported binary encodings with deterministic type validation |
| Prepared statements | Named and unnamed statements scoped to the connection |
| Portals | Named and unnamed portals with per-execute `max_rows`, suspension, resume, cumulative result and retained-memory limits, and cleanup |
| Cancellation | Backend key data plus PostgreSQL cancel requests using process ID and secret |
| Copy ingestion | Supported CSV `COPY FROM STDIN` workflow |

## Mutation and DDL Subset

- Upsert uses `INSERT ... ON CONFLICT`. `DO NOTHING` accepts an optional target; `DO UPDATE` requires an explicit primary-key, unique-constraint, or plain unique scalar-index target and supports existing-row expressions, `excluded.<column>`, parameters, a `WHERE` filter, and `RETURNING`.
- `CREATE TABLE IF NOT EXISTS` and `CREATE [UNIQUE] INDEX IF NOT EXISTS` are name-only no-ops. An existing name succeeds even when the requested definition differs, preserving the existing object and schema epoch. Without the clause, duplicates are errors.
- The index rule applies to Cassie's scalar, full-text, vector, column, hybrid, and time-series index kinds.
- Standalone `UPSERT`, `ON CONSTRAINT`, concurrent conflict arbitration, and partial or expression-index conflict inference are unsupported.

## Errors and Cancellation

Cassie emits PostgreSQL error responses with SQLSTATE codes where a stable mapping exists. Syntax errors use `42601`, unsupported features use `0A000`, undefined objects use their PostgreSQL-family codes, query cancellation and deadlines use `57014`, resource-limit failures use `54000`, and connection admission uses `53300`.

A successful startup emits backend process and secret data. A cancel request affects only the matching live backend. Incorrect or stale secrets do nothing. Cancellation is cooperative at bounded execution checkpoints and cleans up query and portal resources. A cancelled resume returns `57014` and no partial row page.

Portal `max_rows` controls one execute response; it does not reset Cassie's query limits. Result rows are counted cumulatively across resumes, and retained memory is shared across all live portals on the connection. An execute or bind that would exceed a cumulative limit returns `54000` atomically. Closing a portal or statement, rolling back, or disconnecting releases its state.

## Catalog Contract

Cassie supplies the PostgreSQL-like virtual catalog rows needed by supported clients. These views describe Cassie objects; they are not byte-for-byte PostgreSQL catalogs. Applications must not depend on undocumented catalog columns, OIDs, server settings, extensions, or system functions.

## Client Evidence

The repository keeps automated coverage for the native pgwire harness and `tokio-postgres`. Optional probes cover selected psql, SQLAlchemy Core, Prisma, and pgAdmin workflows when their external dependencies are available. A probe documents only the exercised workflow; it does not widen the compatibility contract.

## Intentional Differences

- No full PostgreSQL parity or extension ABI.
- No distributed or serializable transaction promise.
- No trigger or stored-procedure business-logic platform.
- No cross-database queries.
- Cassie-specific search, vector, graph, time-series, projection, and administrative features may use PostgreSQL-compatible syntax without promising PostgreSQL semantics beyond their documented behavior.
