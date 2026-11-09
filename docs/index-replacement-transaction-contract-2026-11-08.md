# Transactional scalar index replacement

This is a source and oracle investigation for #833 and #842, not a completed
support claim. PostgreSQL 18.6 in an isolated temporary-table session accepts
`DROP INDEX` followed by a same-name unique index on another field in one
transaction. The replacement is visible to that transaction. Rolling back to a
savepoint restores the old definition; repeating the replacement and committing
enforces the new unique field and permits values unique only under the old one.
The oracle process was stopped after the probe. The new Cassie tests are authored
but have not been run.

Current Cassie defers index cleanup using a durable table/name record. Its
cleanup resolves the then-current index by that name, deletes its reservation
prefix, and deletes its physical scalar-index data and metadata. A replacement
prepared while the old metadata remains also reuses that old physical storage ID,
and unique reservations default to the same index name. A name-only identity
check on cleanup is therefore insufficient to protect old readers or a new
same-name index. This is a source-backed hazard, not an observed Cassie result.

One candidate gives the replacement a fresh physical index identity and unique
reservation namespace, stages removal of the old name mapping, and persists an
old-identity cleanup record that can retire only the old physical prefixes after
the epoch guard drains. It needs an explicit backward-compatible cleanup-record
and restart contract, atomic old/new catalog/storage publication, savepoint and
failed-statement behavior, concurrent creation/drop fences, index catalog OID
semantics and cancellation-safe cleanup. Existing name-only records and absent or
legacy identities need a safe rule. Published Midge 0.5.2 remains the sole storage
engine; no upstream change, second layer or unproved atomic capability is assumed.
Do not implement a durable identity change until the contract and its concrete
preconditions have been selected and recorded.

## Selected identity direction

For the explicit same-name replacement goal, Jev selected the full
identity-specific design, conditional on the published Midge APIs and durable
compatibility; the earlier deterministic rejection remains a temporary safety
option rather than completion. PostgreSQL 18.6 reported index OIDs 16644 for the
original, 16645 for the staged replacement, 16644 after savepoint rollback, and
16646 for a later committed replacement in the isolated oracle session.

The intended new scalar cleanup owner records the old relation/storage IDs,
catalog identity and exact reservation identity; its cleanup may remove only
that old physical prefix and may remove the name mapping only if the mapping
still identifies the old index. The replacement gets a fresh monotonic storage
identity and a reservation encoding with a disjoint typed key component, not a
magic user index name. The transaction publishes the old cleanup owner, old-name
removal and new metadata/data in the same database family, with earlier preimages
retained for conflict checking. Savepoint and transaction rollback restore the
corresponding overlay; a failed or outcome-unknown commit cannot publish partial
catalog/data. A pinned descriptor carries the old physical identity, and new
catalog identity must distinguish a recreated index while preserving rename
identity. Cleanup runs only after old schema owners drain, under a source-proven
collection/family publication order, and remains idempotent across interruption
and restart.

Legacy name-only cleanup records must remain readable. They do not carry enough
information to distinguish an old index from a same-name replacement: do not
guess or delete a possibly new owner. Their exact recovery/quarantine rule,
downgrade behavior for newly written records, catalog OID allocation and all
cross-path reservation operations must be established before production wiring.
Schema move/drop/backup, conflict lookup, row maintenance, rebuild and pending
publication replay must understand both reservation encodings. Index cleanup
must not race a stale transaction, direct adapter mutation or name reuse. A
published Midge primitive is a blocker only if its actual public API cannot
provide a required guarantee; implementation complexity is not a blocker.

The existing `PendingFieldDrop` record carries legacy scalar reservation names
and physical IDs but not generated reservation identities. Add a defaulted,
omitted-when-empty list for new generation IDs; recovery removes the corresponding
generated-tag prefixes as well as retained legacy-name prefixes. Old records
decode unchanged. Whole-table DROP and table/schema rename must respectively
delete or relocate both tags, and database backup/restore must preserve their
scoped collection identity. Startup replay, interrupted field cleanup and
upgrade/downgrade boundaries need focused witnesses before claiming compatibility.

The selected candidate uses a V3 minimum-reader marker, durably acknowledged in
the schema family before any generated reservation or identity cleanup record is
published in a database family. Marker requests are monotonic: requesting V1 or
V2 cannot lower V3, while V2/V3 requests reassert the maximum marker with the
existing durable compatibility policy, including after an ambiguous outcome.
An aborted or failed later data-family transaction leaves V3 in place; this is a
safe one-way fence, not cross-family atomic rollback. Unknown markers fail closed.
Before enabling the new representation, verify old-reader reopen rejection,
concurrent marker requests, failure between families, recovery and backup/restore.
Per-instance marker locking and bootstrap-only checks do not prove an already
running older process or cloud writer is fenced. Verify a published Midge
exclusive-writer/lease guarantee for each selected storage mode; if it is absent,
reject activation for that exact unsupported mode before any feature mutation.

### Already-running owners and takeover

The Cassie 0.1.0 baseline at `b9afc913` specifies Midge `0.5.1` and its
checked-in lockfile resolves `0.5.1`; the later `e948f025` baseline specifies
and locks `0.5.2`. These are caret requirements, so an unlocked build can use a
later compatible 0.5.x release. Earlier Cassie source also specified `0.4.1`;
it is not evidence that every deployed binary used any one resolved version.
For the installed 0.4.1, 0.5.1 and 0.5.2 sources, a cloud read authority is
constructed only for hybrid storage. The current 0.5.2 transaction read checks
it before and after a blocking read, but local reads do not obtain that cloud
authority. The available source trace therefore does not establish that an old
local reader, paused across lease expiry and takeover, is prevented from
reading or interpreting newly published V3 state. Startup marker admission
alone does not fence an already-running reader.

The 0.5.2 filesystem lease explicitly permits a different holder to take over
after the stored timestamp exceeds the TTL plus skew tolerance, without proving
that the earlier process has exited. Cassie's local and cloud constructors at
`b9afc913` and this worktree use the same `OpenOptions::local(data_dir)` and
`OpenOptions::cloud(cache_path, CloudStorageLocation::new(provider, prefix))`
shapes. The installed 0.4.1, 0.5.1 and 0.5.2 Midge sources use the same local
leader filenames and cloud lease-object name. This is evidence only when both
instances actually resolve to the same underlying local root or cloud control
provider and prefix; it does not prove aliases, provider semantics or takeover
fencing for every prior binary/configuration.

This is separate from writer authority. In 0.5.2, transaction admission checks
the runtime's lease health and monotonic validity; WAL requests acquire writer
effect authority, and filesystem writer effects validate the holder and epoch
under leader-store coordination. Memtable apply also checks health before its
next effect. These source paths are evidence of fencing, not proof of every
mixed-version, in-flight commit or cloud-provider takeover: the exact shared
control location, durable effect ordering, older binaries, expiry and ambiguous
outcomes still need a synchronized witness. The installed crate's injectable
lease clock is behind Midge's `internal-testing` feature, which Cassie's
published dependency does not enable; no public production clock/failpoint is
assumed for the witness. Do not activate V3 generation until both reader and
writer exclusion, or an independently enforceable quiescent migration
boundary, is established for the actual deployment mode and older versions.
The generation representation remains disabled while those conditions are open.
