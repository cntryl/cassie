# Psycopg graph composite qualification

This is an explicit client qualification workflow for the selected graph type
contract (#1268), using Psycopg 3's public `CompositeInfo` and
`register_composite` APIs. Graph support remains Experimental; a checked-in
probe is not an executed qualification result. The source-backed Jev decision
selected this workflow over expanding SQLx's catalog resolver. SQLx's native
graph composite discovery remains unsupported; builtin scalar graph results
have their separate existing tests.

Cassie exposes each composite's type OID and array OID through `pg_type`, and
its fields through `pg_attribute` joined on `attrelid_oid = typrelid` and ordered
by `attnum`. The qualification registers key parts, element references and then
paths, in dependency order, using those actual identities and field types. It
uses Psycopg's own record and array dumpers/loaders; no handwritten binary or
text codec, hardcoded OID or private adapter API is supplied. The selected
workflow supplies catalog metadata explicitly rather than using the broader
`CompositeInfo.fetch()` discovery query, whose PostgreSQL catalog assumptions
are outside Cassie's current finite contract. See the
[Psycopg composite documentation](https://www.psycopg.org/psycopg3/docs/basic/pgtypes.html).

Run the standalone qualification from the repository root:

```sh
python3 -m venv target/graph-client-venv
target/graph-client-venv/bin/python -m pip install -r scripts/qualification/graph-requirements.txt
CASSIE_GRAPH_PYTHON="$PWD/target/graph-client-venv/bin/python" cargo bench --locked --features graph-client-qualification --bench qualification_graph_psycopg
```

The Rust entrypoint starts a memory-mode Cassie instance on an owned ephemeral
loopback listener and passes the port and a generated fixture password to the
Python process through its environment. It awaits the child while serving the
listener on a current-thread async runtime. The child has a 60-second hang
guard, and the listener and child are retired when qualification ends. Missing
Python or dependencies fail this explicit command; ordinary Cargo tests do not
install or invoke Python. The benchmark requires the explicit
`graph-client-qualification` feature, so ordinary `cargo bench --locked` also
skips this host-dependent qualification workflow.

The assertions cover prepared text and binary parameters, text and binary
results, path arrays, zero-hop paths, a one-hop path spanning two vertex
mappings, nested punctuation/backslashes/Unicode, exact bigint extrema and UUID
keys, Describe result OIDs from the live catalog, typed key access and ordered
steps. Invalid path shape must return `22023`, and a later query on the same
connection must succeed. The path parameters are real driver composite objects.
The fixture inserts both bigint endpoint rows, the UUID target and matching
UUID edges before using the existence-checking graph constructors. A graph
query checks both heterogeneous connections, in addition to exact row/key
checks. The whole-path round trips transport paths constructed from those
references. A separate probe reads native heterogeneous one-hop and zero-hop
paths emitted by `GRAPH_TABLE` in both result formats, verifies their composite
type, exact values and Describe OID, then rebinds those returned objects through
prepared text and binary parameters. These probes do not qualify storage image
recovery.

Source identity, actual command outcome and Python/Psycopg versions must be
recorded by the assembler after execution. The new probe is authored/not run
until that receipt exists. Existing clone restore semantics remain unchanged:
restored databases have fresh identities, graph definitions are recreated, and
source references fail `42704`; identity-preserving restore/remapping is not an
offered workflow (#1327).
