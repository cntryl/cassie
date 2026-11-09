# Scoped TIMETZ `to_char` overload diagnostics

PostgreSQL 18.6 rejects `to_char(NULL::timetz, 'HH24')` and a direct
parameter supplied with TIMETZ OID 1266 plus an unknown string or NULL format
at Parse with 42883, the two-argument signature and the standard undefined
function HINT. The error occurs even when either value is NULL. Qualified
`pg_catalog.to_char` names the qualification. Declared routines retain their
existing resolution precedence. A malformed non-NULL TIMETZ literal can fail
with 22007 before overload selection, so this increment does not reject that or
other non-NULL casts, unknown-typed parameters, or other format argument types.

For SELECT and EXPLAIN SELECT, checked wire admission uses the occurrence fold
even when no parameter is supplied; Parse subsequently binder-validates before
publishing the prepared statement. The selected check uses either the explicit
`CAST(NULL AS TIMETZ)` AST or a direct parameter whose immutable original wire
OID is 1266; it does not globally mark OID 1266 as a qualified primitive. The
matching simple/binder path is restricted to the same forms and respects visible
declared routines. Other statement and overload shapes remain open.

The crate-private semantic diagnostic retains the exact bounded signature and
standard static HINT. Its wire path admits the copied hint together with the
message and complete response frame; the frame owns that reservation through
writing and cancellation. Generic binder/embedded diagnostic allocation and
transport limits remain separately unqualified. Jev selected this narrow route
(0.68) conditional on original-wire-OID, error precedence, routine resolution,
HINT admission/lifetime and recovery evidence. All authored Cassie probes are
unexecuted pending the consolidated build/test gates.
