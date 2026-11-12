# Staged point assertions: retained-horizon prerequisite

`AdapterTransaction::assert_value` currently registers an assertion only on its
inner native transaction. A staged adapter commit drops that inner transaction,
retaining only `local_changes` in `StorageOverlay`; pure assertions disappear.
`commit_storage_overlay` reasserts mutation preimages and writes changes, and
returns early for an otherwise empty overlay. Thus it is not yet a general
transactional point-assertion API.

The published `cntryl-midge` 0.5.2 source documents that native `assert_value`
registers its condition without observing pending writes. At native commit it
checks every asserted expected value against the transaction's frozen start
snapshot, then checks that the key has not changed since that start. Repeated
contradictory assertions are not equivalent to overwriting the first value.
Jev selected a separate native-preimage assertion owner as the design direction
(`target/assembly-review/overlay-point-assert-contract-*` and
`overlay-native-assert-semantics-*`), conditional on full semantics, ownership,
savepoint/COPY/RR cloning and final assertions before writes.

Reasserting the same expected bytes in a fresh final transaction is weaker: a
key can change A→B→A between staging and final transaction start, then compare
equal despite the intervening mutation. Midge 0.5.2 exposes neither an
assert-since/start-sequence API nor savepoint/removal of native assertions.
Holding one native transaction cannot undo assertions on SQL savepoint
rollback, and committing separate native assertion transactions before the
actual final write would not be atomic.

Cassie's retained `publication/row_history` records only authoritative row,
document and column-store keys, not metadata keys. Its tracked admission also
disallows native writable handles for the storage instance after admission and
rejects tracked admission after one has escaped. Expanding that mechanism to
metadata would impose the broader native-interop restriction previously
rejected for this task, and would not fence a still-running older process.

The exact ABA-safe staged assertion guarantee is therefore pending on a
published horizon/savepoint primitive or another fully proven coordination
contract. No value-only substitute is called native-equivalent or used to
activate #843 extension lifecycle. Any future narrower value-preimage API must
state its caller's ABA tolerance and separately prove memory admission,
cancellation, savepoint restoration, contradictions, failure timing,
OutcomeUnknown quarantine and final assertion-before-write ordering. No runtime
implementation or Cassie test execution is claimed here.
