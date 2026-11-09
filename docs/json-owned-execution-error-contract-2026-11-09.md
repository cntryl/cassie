# Private owned execution-error path

This is a design record for the #809 row-JSON diagnostic handoff, not a
production implementation. Baseline `f6a757d3`; no build or test was run.
Jev selected investigation of a private owner-carrying execution-error path
(0.95), with internal conversion/worker flow (0.76), transport lifetime
(0.33), embedded public memory (0.67), and 42601/display/descriptor parity
(0.46) as leads. Receipt:
`target/assembly-review/json-owned-error-api-contract-{request,response}.json`.

`CassieError::Parse(String)` displays `parse error: {message}`; its internal
descriptor returns SQLSTATE 42601 with the bare message. The independently
cloneable internal descriptor String and the public Parse String cannot retain
a query reservation. Both
`CassieError` and the re-exported `QueryError` are public enums, so adding a
variant breaks downstream exhaustive matches. The existing
`OwnedNumericLiteral` variant is specifically numeric and is not reused for
row JSON. `Cassie::execute_sql` and its cancellation variant retain their
current public `Result<_, CassieError>` signatures and serialization/API shape.

The proposed internal path uses a private result/error type with an owned row
diagnostic paired with its reservation, alongside existing Cassie errors.
The owned value must remain intact from controlled TYPE_JSON decoding through
scan iterators, worker/channel results, executor and app query entrypoints to
the final consumer. Today row decode and controlled scan closures return
`CassieError`; `execute_sql_core` and `execute_sql_with_controls` also return
that type. Simple pgwire `handlers/query.rs` currently calls public
`execute_sql`, while extended portals call
`execute_parsed_sql_with_cancellation`. A local decoder change alone therefore
cannot preserve an owner to the wire. Exact affected paths and thread/error
transfers must be audited before implementation.

For pgwire simple, extended and portal paths, the proposed private app entry
must expose the owned diagnostic to checked error-frame construction, retain
both source and encoded frame reservations through the actual write boundary,
and preserve requested severity, SQLSTATE and bare message. REST needs an
equivalent private error route and admitted body construction; existing
Hyper/header/TLS/transport buffering boundaries remain separately unqualified.
No channel may silently replace a row parse error with a numeric error or
change malformed-input ordering. Cancellation/resource failures remain their
original errors.

At the existing public embedded API boundary, converting an owned diagnostic
to `CassieError::Parse(String)` necessarily produces a detached public String;
its post-return lifetime and clones are explicitly unqualified unless a
compatible owner contract is proved. The same applies to the existing freely
cloneable descriptor escape. This limitation cannot be hidden by charging a
temporary construction reservation that is dropped before the returned String.
Midge remains the direct storage layer; this design makes no engine API change.

Before activation, author focused old/new error text, SQLSTATE and severity
probes; malformed syntax/UTF8 precedence; exact/below-budget and synchronized
error construction/frame-copy cancellation; worker/channel and fallback error
drop ordering; pgwire simple/extended/portal write lifetime; REST body handoff;
and embedded conversion/clone behavior. Prove every private conversion is
fallible and owner-preserving, audit all error matches and network paths, and
qualify the row parser and adapter/consumer representation separately. No
production activation, end-to-end memory claim, recovery claim or executable
pass follows from this record.
