# Cassie SQL Compatibility Profile

This document records the boundary for PostgreSQL-shaped SQL and client
compatibility in Cassie. The [Product Roadmap](product-roadmap.md) owns product
scope and release order. [Feature Support](feature-support.md) owns current
behavior and support status; [PostgreSQL Compatibility](postgres-compatibility.md)
owns pgwire and client behavior.

PostgreSQL 18.6 is a semantic reference for selected behaviors. Its grammar,
catalogs, test suite, and client ecosystem are not a complete Cassie feature
list. Cassie does not target full PostgreSQL server, core-SQL, extension, or
PL/pgSQL parity.

## Active product profile

- **v0.1.0 Projection Store:** selected projector writes, read-model types and
  schema lifecycle, rebuild operations, and the driver/ORM/migration workflows
  named by the live milestone issues.
- **v0.2 Query and retrieval:** selected relational query, type, function,
  full-text, vector, and hybrid behavior named by the roadmap and live milestone
  issues.
- **v1.0 Production:** correctness, resource, restart/recovery, client, and
  operational qualification of the documented Cassie profile.

An individual PostgreSQL-shaped statement is in scope only when its behavior is
documented in the roadmap or current feature contract and has an owning live
issue. That issue defines exact syntax, types, NULL and error behavior,
parameters, wire format, resource limits, and lifecycle expectations. Similar
syntax or a passing comparison case does not imply support for neighboring
forms.

## Architectural boundaries

- Cassie is single-node. Midge remains its only direct storage layer.
- PostgreSQL wire is the primary query interface. REST is secondary and
  administrative.
- External systems own routing, placement, failover, replication, and fleet
  coordination.
- Stored programs, triggers, arbitrary extension loading, and unlisted
  PostgreSQL server behavior are outside the product unless a future roadmap
  explicitly selects a bounded capability.
- The planned [#843](https://github.com/cntryl/cassie/issues/843) `vector` and
  `pgcrypto` compatibility shims are finite SQL/catalog behavior; they do not
  load extension libraries or claim extension ABI parity. Vector operations
  remain bounded by [#844](https://github.com/cntryl/cassie/issues/844).
- A syntax accepted for a driver or migration workflow does not imply the full
  PostgreSQL semantics behind that syntax. Compatibility shims must follow the
  exact contract of their owning issue.

## Qualification

Compare only selected in-scope cases against an immutable PostgreSQL 18.6
reference. Record values, multiplicity, declared order, NULL behavior, result
metadata, SQLSTATE, command tags, and parameter behavior as applicable. Keep
the reference artifact and test runner versioned; do not treat corpus size or
test presence as a support claim.

Release and production gates come from the [roadmap](product-roadmap.md),
[release checklist](release-checklist.md), and
[production readiness record](production-readiness.md). The former #791–#793
complete PostgreSQL parity program was closed as superseded. Remaining
unselected parity work is not an active release dependency.

## Historical material

The former full-parity proposal and its dated implementation notes are retained
in [sql-dialect-profile-history.md](sql-dialect-profile-history.md) for
provenance. Those historical plans do not override the active profile above.
