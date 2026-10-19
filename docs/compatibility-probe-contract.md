# Compatibility Probe Contract

> **Current operations constraint:** GitHub Actions are disabled by user direction. Do not
> enable or dispatch the archived external probe workflow until the user explicitly changes
> that direction. Local probes remain available, subject to their documented prerequisites.

Compatibility probes are opt-in external gates. They are not part of pull-request CI and a
successful probe certifies only the recorded workflow, client version, source revision, and
runtime profile.

## Dispatch

The [archived workflow at the frozen baseline](https://github.com/cntryl/cassie/blob/5f9bfd807d4e647401fbb8a587429c48425af61e/.github/workflows/compatibility-probes.yml) records this external dispatch interface. Current local coverage runs through `cargo test --locked --test compatibility`; externally provisioned runners retain the manifests specified below. The archived inputs are:

- `probe`: `native`, `sqlx`, `diesel`, `prisma`, `sqlalchemy`, `psql`, `pgadmin-9.16`,
  `dbeaver-26.1.3`, or `catalog`;
- `run_external`: `true` only when the required client tools are available;
- `source_revision`: the full Cassie commit SHA being tested; the workflow fails unless it exactly
  matches the checked-out `HEAD`.

The archived workflow never accepts a password or other secret input. It starts Cassie with the test-only
in-memory fallback and uses loopback connections. External runs must retain only normalized,
secret-free stdout/stderr and the generated manifest.

## Pinned clients

| Probe | Pinned client/tool | Current repository status |
| --- | --- | --- |
| sqlx | sqlx `0.8.3`, PostgreSQL driver `0.8.3` | Dedicated opt-in fixture; support is limited to the workflow below and requires retained passing evidence |
| Diesel | Diesel `2.2.6`, PostgreSQL backend `2.2.6` | Dedicated opt-in fixture; support is limited to the workflow below and requires retained passing evidence |
| Prisma | Prisma CLI `6.1.0` | Opt-in probe in `compatibility_matrix.rs` |
| SQLAlchemy | SQLAlchemy `2.0.36`, psycopg `3.2.3` | Opt-in probe in `compatibility_sqlalchemy.rs` |
| psql | `postgres:16.6-bookworm` image | Opt-in probe in `compatibility_matrix.rs` |
| pgAdmin | pgAdmin `9.16` | Pinned normalized trace replay; live desktop gate is not provisioned or certified |
| DBeaver | DBeaver `26.1.3`, PostgreSQL JDBC `42.7.11` | Pinned normalized trace replay; live desktop gate is not provisioned or certified |

## Desktop trace replay

The `pgadmin-9.16` and `dbeaver-26.1.3` lanes validate the checked-in fixture digest, client and
driver identity, upstream source revision, required workflow coverage, SQL or protocol mode,
expected columns, row count, and deterministic SQLSTATE failures before replaying the fixture
against a temporary authenticated Cassie pgwire listener. The retained manifest records
`status=replay-passed` and `certification_status=unavailable`; trace replay is a prerequisite and
never a live-client certification claim.

Live password/TLS connection and reconnection, cancellation, native navigator/properties UI,
graphical plan rendering, and primary-key grid editing still require separately provisioned
pgAdmin and DBeaver desktop runners. No such repository runner is currently available, so those
gates and exact-revision live artifacts remain open.

## sqlx workflow

The `sqlx` lane builds the isolated, locked fixture in `tests/fixtures/sqlx_probe` and connects
over loopback pgwire. It verifies supported information-schema discovery, prepared positional
parameters, a committed parameterized write, rollback visibility, and deterministic `23505`
and `42P01` SQLSTATE mapping. The retained manifest records the requested Cassie revision and
the exact `rustc` toolchain; it contains no connection URL or credentials.

This probe does not certify sqlx migrations, compile-time query macros, COPY, LISTEN/NOTIFY,
PostgreSQL extensions or internal catalogs, non-default isolation modes, replication, or full
PostgreSQL compatibility. A client-version change requires a new retained probe run.

## Diesel workflow

The `diesel` lane builds the isolated, locked fixture in `tests/fixtures/diesel_probe` and
connects over loopback pgwire. It verifies supported information-schema discovery, bound SQL
queries, a committed write, rollback visibility, unique-violation mapping, and deterministic
missing-relation behavior. The retained manifest records the requested Cassie revision, exact
`rustc` toolchain, Diesel version, profile, protocol mode, and outcome without credentials.

This probe does not certify Diesel migrations, schema generation, Diesel's typed query DSL,
COPY, LISTEN/NOTIFY, PostgreSQL extensions or internal catalogs, non-default isolation modes,
replication, or full PostgreSQL compatibility. A client-version change requires a new retained
probe run.

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
