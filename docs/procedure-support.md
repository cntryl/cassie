# Limited Procedures and CALL

Cassie's procedure surface is a narrow PostgreSQL-wire compatibility and administration contract.
It is not a stored-procedure business-logic platform.

## Supported Contract

- `CREATE PROCEDURE name(arguments) AS "body"`, `CALL name(arguments)`, and
  `DROP PROCEDURE` are supported.
- A body is exactly one single Cassie SQL statement. The statement is parsed and bound when the
  procedure is created, and executed through the same resource controls and authorization checks
  as a direct statement when called.
- Arguments have names and Cassie SQL types. Calls require the declared arity and use positional argument binding:
  `$1` refers to the first declared argument, `$2` to the second, and so on.
- Procedure metadata and bodies persist through restart hydration. A procedure created before a
  clean restart remains callable afterward.
- Pgwire reports normal command metadata for the supported subset. The retained
  `tokio-postgres` compatibility test creates a procedure, invokes `CALL` with a bound parameter,
  and reads the resulting row.

## Unsupported Contract

Cassie deterministically rejects or does not parse these surfaces:

- PL/pgSQL and every other procedural language;
- triggers and trigger invocation;
- dynamic SQL such as procedural `EXECUTE`;
- transaction control inside a procedure body;
- direct or indirect recursion; and
- multi-statement application workflows or any general business-logic platform.

These are intentional product boundaries, not missing PostgreSQL syntax scheduled implicitly for
implementation. Applications should keep orchestration and business logic in application code and
use transactions through the normal pgwire statement flow.

## Evidence

- `tests/executor_commands.rs` covers argument substitution, execution after restart, transaction
  control rejection, and recursive-call rejection.
- `tests/compatibility_matrix.rs` covers the supported create/call/read workflow through
  `tokio-postgres`.
- `tests/procedure_support_contract.rs` keeps this published boundary explicit and verifies that
  PL/pgSQL, triggers, and dynamic SQL remain unreachable.
