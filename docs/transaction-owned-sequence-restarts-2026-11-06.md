# Transaction-owned sequence restart epochs (#833, #839)

Selected contract before implementation: each transaction-created sequence has
an identity-keyed active cursor epoch. Ordinary `nextval`/`setval` advance the
active epoch nontransactionally, including when a statement fails or a
savepoint rolls back. A successful transactional `ALTER ... RESTART` or
`TRUNCATE ... RESTART IDENTITY` forks a fresh cursor epoch. The predecessor
retains its cursor at the instant of restart, including ordinary advances made
since an earlier savepoint. Savepoints restore the epoch selection, so rolling
back a restart restores that predecessor; it does not restore the cursor to its
value at SAVEPOINT creation. `currval` and `lastval` remain session-owned and
continue to report the last actual sequence call across that rollback.

The selected implementation keeps epoch selection in the cloned transaction
storage overlay and keeps cursor values in its shared private owner, keyed by
allocated object identity and unique epoch. Statement forks get independent
epoch selections and publish them with their existing row/catalog stage only
on success; a failed or cancelled restart never changes the parent's selected
epoch. Retain exactly epochs reachable from the current overlay or any retained
savepoint, and admit their directory/backing under the existing stable
transaction cursor budget before allocating or applying a restart. Overflow,
resource admission, cancellation, serialization, or failed statement publication
must leave the previous selected epoch intact and expose no partial result.
Commit merges only each live identity's currently selected epoch into the same
existing Midge family overlay. Rename retains identity, while drop/recreate
cannot reuse an older identity or its epochs. Published sequence operations
retain the existing native conflict/retry and transactional restart paths. No
Midge source, Midge capability, durable format, public Rust API, or additional
storage layer changes.

Jev selected the per-identity, savepoint-aware epoch owner with probability
0.94; its 0.91 risk requires nested/released restarts, intervening ordinary
advances, failure/cancellation, recreation/rename and admission/lifetime
probes. PostgreSQL 18.6 observed `nextval=1; SAVEPOINT; nextval=2; RESTART 10;
nextval=10; ROLLBACK TO; currval=10; lastval=10; nextval=3`. A nested probe
observed epoch 10 advancing through 11, epoch 20 returning 20, rollback to the
inner savepoint returning to epoch 10 at 12, restart 30 and RELEASE, then
rollback to the outer savepoint returning to the original epoch at 3. An
invalid restart below MINVALUE returns `22023` and preserves the prior epoch.
`TRUNCATE ... RESTART IDENTITY` likewise restores the predecessor after rollback.
Raw oracle receipts are in the temporal worktree's ignored
`target/scalar-oracle/staged-sequence-restart*.txt`; the Jev receipt is under
the assembler's ignored `target/assembly-review/sequence-epoch-*.json`.

The existing COPY ownership gap and published renamed-sequence route remain
separate pending work. The regressions for this increment are authored before
runtime edits and remain unexecuted until the assembled build and test gate.

PostgreSQL 18.6 also forks sequence state for transactional parameter changes:
`nextval=1; SAVEPOINT; nextval=2; ALTER IDENTITY SET INCREMENT BY 5; nextval=7;
ROLLBACK TO; nextval=3`. The epoch boundary therefore covers successful private
sequence parameter changes, not only explicit restart. The authored regression
records this as unexecuted candidate coverage.

Cloned session handles may execute concurrently. A statement fork therefore
registers an admitted RAII lease for its reachable identity/epoch pairs before
releasing the originating transaction lock. Any new pair created or advanced
inside the fork is registered before it can become observable. Pruning retains
all pairs protected by a live fork even if parent and savepoint overlays no
longer select them. The guard survives successful publication, failure and
cancellation until the last fork handle retires; the published parent copy does
not retain a stale fork guard. Duplicate leases use reference counts, and both
directory and pair allocations belong to the existing transaction-owned cap.
Jev selected this bounded owner (0.96) over scanning weak overlay references
(0.02); the required risk probes cover atomic registration, fork-only epochs,
publication and cancellation, lock ordering, duplicate refcounts and release.

This source candidate supersedes the interim `0A000` guard documented in the
preceding cursor increment for the selected private ALTER/RESTART cases. That
change is not qualified until the assembled validation gate completes. The
pre-existing complete storage-overlay snapshot clone remains a separate
admission audit obligation; the new epoch-selection and fork-lease allocations
are accounted independently, including coexistence with retained snapshots.
