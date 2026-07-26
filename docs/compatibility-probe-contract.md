# Compatibility Probe Contract

Compatibility probes are opt-in external gates. They are not part of pull-request CI and a
successful probe certifies only the recorded workflow, client version, source revision, and
runtime profile.

## Dispatch

Run `.github/workflows/compatibility-probes.yml` manually with:

- `probe`: `native`, `sqlx`, `diesel`, `prisma`, `sqlalchemy`, `psql`, `desktop`, or `catalog`;
- `run_external`: `true` only when the required client tools are available;
- `source_revision`: the Cassie commit being tested.

The workflow never accepts a password or other secret input. It starts Cassie with the test-only
in-memory fallback and uses loopback connections. External runs must retain only normalized,
secret-free stdout/stderr and the generated manifest.

## Pinned clients

| Probe | Pinned client/tool | Current repository status |
| --- | --- | --- |
| sqlx | sqlx `0.8.3`, PostgreSQL driver `0.8.3` | Planned; no claim until a dedicated sqlx fixture passes |
| Diesel | Diesel `2.2.6`, PostgreSQL backend `2.2.6` | Planned; no claim until a dedicated Diesel fixture passes |
| Prisma | Prisma CLI `6.1.0` | Opt-in probe in `compatibility_matrix.rs` |
| SQLAlchemy | SQLAlchemy `2.0.36`, psycopg `3.2.3` | Opt-in probe in `compatibility_sqlalchemy.rs` |
| psql | `postgres:16.6-bookworm` image | Opt-in probe in `compatibility_matrix.rs` |
| pgAdmin | pgAdmin `9.16`, JDBC `42.7.11` where applicable | External desktop gate; not certified here |
| DBeaver | DBeaver `26.1.3`, JDBC `42.7.11` where applicable | External desktop gate; not certified here |

## Evidence

Each retained manifest records `probe`, client/tool versions, Cassie source revision, Rust or
runtime toolchain, deployment profile, workflow step, SQL or command shape, protocol mode,
expected columns and row shape, status (`passed`, `failed`, or `unavailable`), and normalized
diagnostics. Credentials, connection strings, and machine-specific paths must be redacted.

The supported contract is limited to connection, documented catalog/read-model queries,
parameters, prepared queries, bounded transactions, cancellation where implemented, and
deterministic SQLSTATEs. Unsupported isolation modes, PostgreSQL-internal catalogs/extensions,
GUI create/alter dialogs, migration parity, replication, and distributed behavior remain
explicitly outside the claim.

