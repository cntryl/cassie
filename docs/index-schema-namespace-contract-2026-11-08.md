# Index schema namespace

This #842 investigation concerns CREATE INDEX naming and does not establish
transactional DROP/recreate or the broader index milestone. PostgreSQL 18.6
returned `42P07` for a duplicate index name on a second table in the same
schema, accepted the same local name in another schema, and returned `42P07`
when a table or sequence already occupied the proposed name. A valid `IF NOT
EXISTS` collision skipped creation, but a nonexistent indexed column still
failed first with `42703`. An unnamed index avoided another table's
`genrows_v_idx` and chose `genrows_v_idx1`. The isolated oracle was stopped;
new Cassie probes are authored but unexecuted.

A second PostgreSQL 18.6 probe confirmed that `IF NOT EXISTS` validates an
unknown access method (`42704`), unknown storage parameter (`22023`), missing
INCLUDE column (`42703`) and missing predicate column (`42703`) before skipping
the colliding name. View and materialized-view names also collide (`42P07`).
Quoted `"Case.Index"` and `"case.index"` coexist in the same schema. These are
oracle observations, not claims that Cassie currently supports all those
options. The isolated oracle was stopped after the probe.

Current `bind_create_index` resolves duplicate and generated names with
`Catalog::get_index(table, name)`, which is table-scoped. It also returns from
`IF NOT EXISTS` before indexed-column validation. A potential repair must check
the target schema's complete relevant relation namespace, preserve quoted names,
schema/database isolation, materialized-view visible identities and SQL error
precedence, and choose names against the same namespace. It must check staged
transaction visibility and savepoint behavior, and avoid a binder-only race with
concurrent publication or direct embedded catalog/storage writes. User-facing
SQLSTATE, cross-phase catalog ownership and supported relation kinds require a
focused decision and source review before implementation. Jev selected a shared
logical schema relation-namespace resolver, conditional on proof of atomic
catalog publication and the quoted/database/materialized-view cases. Same-name index
replacement while an old cleanup is pending remains separate and unqualified.

Case-distinct quoted names remain a precise pending boundary: the current index
OID fallback hashes a lowercased name, existing lookup paths fold case, and
sequence registration also uses lowercased keys. Jev selected investigating a
complete persisted OID and lookup policy before assigning new identities; the
existing optional OID field alone is not proof of collision freedom or upgrade
compatibility. Until those paths are coherent, reject a proposed index whose
schema-qualified name differs only by ASCII case from an existing relation with
`0A000`. This is a conservative Cassie-side punt, not PostgreSQL parity or a
reason to close #842. Exact duplicates still use `42P07` or the supported
`IF NOT EXISTS` skip after validation.

For generated names, PostgreSQL 18.6's `makeObjectName` in
`src/backend/commands/indexcmds.c` apportions the 63-byte identifier budget
between the table and column components, shortening the longer component (the
column component on ties), then clips each at a character boundary. Collision
labels are `idx1`, `idx2`, and so on. Expression-derived names are not yet
qualified here. The unscoped public binder must continue to handle both bare
and schema-qualified legacy catalog names using its binding database; lookup
errors must propagate rather than becoming an endless generated-name retry.
An isolated PostgreSQL 18.6 probe with a 40-byte ASCII table name and a
40-byte ASCII column name produced 29 bytes from each component plus `_idx`
(63 bytes total); its server was stopped afterward.

The draft checks originating controls while parsing and comparing names, keeps
admitted collection/index name snapshots, and checks projection/version scans.
Its phased consistency, cancellation at synchronized scan boundaries, quoted
catalog identity across restart, and raw native storage writers require proof.
Controlled map acquisition uses interruption-checked timed waits; ambiguous
materialized projection owners fail closed. Authored lock-wait, owner-release,
and duplicate-output probes are unexecuted.
The draft cannot be treated as a resource-qualified or completed #842 fix.
Unparseable stored names cause a conservative failure rather than being skipped.
For compatibility, unquoted index lookup retains its existing case-insensitive
fallback. A quoted local name whose rendered path retains its quotes requests
an exact match, but `IdentifierPath` can discard optional quotes from a simple
mixed-case identifier. Full quote-preserving index lookup remains pending; this
is not a PostgreSQL case-sensitive catalog identity implementation.

The selected controlled scan uses originating query controls through the
internal binder and executor, checked identifier parsing, and borrowed catalog
entries. Existing public binder calls remain available without query controls.
It must avoid holding projection/index locks in opposing orders. A catalog
version check alone is not a complete fence for direct public mutations:
`unregister_projection_metadata` drops its temporary write guard before its
subsequent version increment. Direct Catalog mutation during the scan and its
interaction with SQL publication remain pending until a real fence is proven;
do not claim atomicity from a before/after version comparison. Jev selected
this direction in `target/assembly-review/index-namespace-controls-gate-*`;
its judgment does not establish the lock or publication proof.

The focused fence decision (`target/assembly-review/index-namespace-fence-gate-*`)
selects deferring race-sensitive publication until it is proven. A consistent
scan alone cannot close the later CREATE INDEX check-to-Midge-to-catalog-write
interval. A candidate mutation-wide, reentrant catalog fence would need to
cover collection, view, sequence, index, and materialized projection public
mutators, `clear`, collection rename, and transaction snapshot publication, as
well as hold across final SQL revalidation and publication. It would also need
to preserve existing collection-gate order: vector backfill and scalar index
rename already hold collection gates when updating the catalog, whereas taking
the proposed fence before CREATE INDEX calls Midge would reverse that order.
Detached transaction catalog snapshots and final base-version validation need
the same proof. This is unresolved Cassie-side work, not a claim about an
unavailable Midge API, and the current draft must not be integrated as atomic.
