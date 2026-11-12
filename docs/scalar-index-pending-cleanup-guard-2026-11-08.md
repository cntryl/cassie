# Scalar index pending-cleanup guard

This source-only increment addresses the unsafe interval between a scalar index
DROP and its deferred name-only cleanup. It does not implement same-name
replacement in that interval. A CREATE for the exact table/index name must fail
before preparing metadata or rebuilding index data while a matching cleanup is
staged or persisted; after cleanup completes the name is available again.
Different names and tables remain eligible. The selected diagnostic is a
deterministic `Unsupported` error containing `pending index cleanup`; it must
leave both the marker and index state untouched. The planned SQL probes are
authored but have not been run.

The older durable cleanup record identifies the table and index by name, so
without the guard a later cleanup can remove the replacement's metadata and
reservation prefix. An old schema epoch may keep the cleanup pending across a
commit. Rolling a staged DROP back to a savepoint or rolling back the transaction
must remove its uncommitted cleanup owner. Name-only legacy records are included.
Multiple distinct cleanup IDs may refer to the same index name; the name is not
available until all matching owners are gone, even after one has completed.
The desired success case, an atomic same-name DROP/CREATE before cleanup drains,
remains unsupported until the durable generation and live old-reader fence in
the separate index replacement contract can be established.

The guard must cover both the scoped transaction view and the persisted current
database, direct adapter publication and pending-publication replay. A collection
gate must serialize the final check with metadata publication and cleanup. A
cleanup worker must re-read its marker under that gate before deleting anything:
two workers can otherwise scan the same marker, and a stale second worker can
delete an index recreated after the first removes it. Transaction commit must
also protect and revalidate metadata-only collections, even when no row writes
cause the existing transaction gate selection to include them. The exact
collection set and lock ordering are being traced before implementation; this
paragraph is a safety requirement, not evidence that the route is complete.

For a staged DROP, the UUID cleanup marker itself does not assert that the index
metadata it names remained unchanged. The selected restricted design records the exact index
metadata key as an unchanged staged write in the same data transaction as the
marker; the overlay already retains its first expected value across later
rewrites, and final publication asserts that value before applying changes.
It must capture the resolved metadata through a fresh current view, fail closed
if the exact key/identity cannot be established, and respect savepoint rollback.
Final guard classification must distinguish that unchanged witness from a real
CREATE/update, and permit a newly created then dropped index where the original
key was absent. It must still reject any conflicting separately persisted marker
that appeared after staging. If the stored target or its original expectation
cannot be established, fail closed; the limited guard may conservatively reject
otherwise valid combinations. Jev selected this classification subject to exact
target binding and guarded publication; neither implementation nor qualification
is implied by that decision.

For an index created then renamed in one transaction, the old staged key can be
deleted with a Missing preimage. The draft derives that source's collection from
another retained staged index record, requires an exact encoded collection
prefix, decodes its remaining name and re-encodes the complete key before
including the source in commit gates and persisted-marker/journal checks. It
fails closed when no unambiguous collection witness survives. Quoted/dotted
names, repeated renames, delete-after-rename and savepoints have authored,
unexecuted probes. The full key/UTF-8 scan and encoder scratch are not yet
qualified for interruption or pre-admission.

The current `AdapterTransaction::scan` sees a staged overlay in a scoped
read-only request because `begin_scoped_adapter_tx` seeds those changes into the
native transaction first. The existing all-database cleanup scan cannot be used
as the guard inside that scope: accessing a different database family is rejected.
The pending check must instead query the target's database and account for
malformed records, cancellation and ownership without guessing a missing match.
The draft guards use the existing serialized-JSON decode estimate and a checked
reader, retaining decoded owners through matching. This does not establish a
complete serde allocation bound or interrupt individual post-read UTF-8/key
operations; native Midge's scan also does not expose pre-admission for its raw
returned bytes. These qualifications and exact/below-budget witnesses remain
open rather than being treated as completed resource guarantees.
Midge 0.5.2 is the sole published storage engine; the guard must use its existing
API and does not authorize a new on-disk format.

## Escaped native writer boundary

This draft does not establish a safe same-name index guard against all supported
embedded writers. `Midge::data_tx` and `Midge::database_tx` return a public
`cntryl_midge::Transaction` whose later read-write commit bypasses Cassie's
collection and database-publication gates. A retained native writer can add a
previously absent, UUID-keyed cleanup or publication record after the guard
scans and before its publication. Published Midge 0.5.2 exposes `assert_value`
for an exact key but no supported assertion that a scanned prefix remains empty.
Holding Cassie's database-publication gate can coordinate its `raw_put` and
adapter transactions, but cannot fence the escaped native commit.

Cassie's existing per-instance tracked-RR/native-writer admission could be
extended into a lifetime-wide exclusion, but Jev selected preserving the current
native read-write interop (0.99) rather than introducing that broader restriction.
The unfenceable phantom case is therefore punted: this draft is not a completed
or qualified safety guarantee and must not be integrated as one. It does not
change the earlier reviewed stale-cleaner repair. An upstream published
range/prefix assertion or a separately authorized enforceable interop contract
would need its own design and executable proof. This is distinct from the
already-running old-reader/layout-fence limitation of the unactivated V3 design.

Jev selected a fail-closed prepared-journal rule: a DROP must not create a
cleanup marker while a same-name publication awaits recovery, and a new direct
publication rejects before mutation when it observes any existing index journal;
the existing journals retain their sorted recovery path before a retry. This
restriction applies only while unresolved publication work is observed; the
target gate does not establish global ordering against concurrent publications
on other collections. The target collection
gate spans the early check, preparation and that target's publication; it is
released after that target is published. Jev selected returning success at
that boundary rather than automatically replaying a different collection's
journal that appeared concurrently: a replay error after durable target
publication could otherwise cause SQL to skip registering the published index.
The later unrelated journal remains for sorted recovery; for a running Cassie,
a clean restart runs that recovery before catalog hydration. The public Midge
replay method alone updates storage and does not refresh a live Cassie catalog.
This changes recovery timing and requires synchronized catalog/durable, failure
and ambiguous-outcome witnesses. A matching
cleanup/journal pair must fail closed with an actionable ambiguous-recovery
diagnostic rather than direct the operator into a replay cycle. There is no
automatic identity-safe resolution for a legacy ambiguous pair: preserve the data
and both records, and restore a validated backup or investigate the original
index owner before any manual repair. Whether a cleanup/journal cycle can prevent
recovery, and the cross-database scoped-overlay behavior remain open. Failure
injection, restart, vector and rename witnesses are required before integration.
