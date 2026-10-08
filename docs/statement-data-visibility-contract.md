# Statement Data visibility contract

## Selection and implementation status

[#763](https://github.com/cntryl/cassie/issues/763) selects one shared
same-database Data statement view (choice B), following the user decision.
This is the implementation contract, not a claim of completed support.
At baseline `0a36504091287303b390182a031571793ea66f1f`, separately
opened read owners acquire separate Midge transactions. End-to-end sharing,
race qualification and release acceptance remain pending. Prerequisite
[#755](https://github.com/cntryl/cassie/issues/755) is closed.

## Acquisition and visibility

After binding and authorization, before the first authoritative Data read,
a statement captures one readonly Midge transaction for its logical database
and one immutable copy-on-write snapshot of its session's staged writes.
Every selected read owner in that database uses those same captured owners.
A commit after capture cannot change the statement's committed rows, index
membership or Data metadata; a staged-write change after capture cannot
change its overlay. Read-path eligibility checks use that captured map, including
an empty captured overlay; another session keeps its live staged-write authority.
Mutation staging and commit checks retain their existing live state.
Empty inputs obey the same acquisition law. A subsequent
statement captures a fresh view and can observe later committed state.

Existing embedded executor entrypoints use the same acquisition law when called
directly. An internally supplied matching owner must be installed for that
execution; an ordinary direct call captures a fresh local owner before its first
Data read. Reusing caller controls alone does not select transaction-wide
repeatability. Command mutation authorities remain outside the read scope.

This law covers row sources, joined sources, repeated CTE and subquery reads,
worker execution, index candidate and row retrieval, column metadata and
chunks, specialized retrieval, and their scalar or row fallbacks. A fallback
must not silently open a newer Data view. Existing ordering, equality,
visibility, authorization and descriptor laws remain authoritative.

The view covers exactly one logical database's Data physical family.
Schema/catalog and Temp visibility remain separate. Cross-database
synchronization, transaction-wide repeatable snapshots, REPEATABLE READ,
SERIALIZABLE and transactional DDL are not selected. Existing cross-database
SQL support is not promoted to an atomic cross-database snapshot guarantee.
Midge remains the only storage layer; no persistent layout, dependency
upgrade or public API change is selected.

## Mutation and commit authority

DML acquisition is limited to source materialization: INSERT SELECT, UPDATE
matched rows and predicates, and DELETE matched rows and predicates. Their
joins and EXISTS reads share that owner. The source scope ends before mutation
preparation, uniqueness/FK checks, write staging and commit validation, which
keep their existing authorities.

A DML statement's read-source selection uses the statement view. Its private
mutation staging, constraint checks and write transaction retain their
existing authorities. A statement read owner must never substitute for the
fresh gated commit state used to validate foreign keys and parent references
(TX-08). Existing mutation rollback, multi-collection atomicity, failed-block,
savepoint, session-setting and post-durable maintenance laws (TX-01 through
TX-09) remain required. Read sharing cannot hide writes from the existing
same-statement mutation or constraint machinery.

## Portals and termination

Parse, Bind and Describe alone do not capture a Data statement view. The
first execution captures the view and overlay. A suspended cursor retains
them; a materialized portal retains its immutable result. Resume must not
reopen the cursor's Data transaction or recapture the overlay. Existing
commit, rollback, Close, statement-close cascade, disconnect and implicit
segment boundaries terminate owners; explicit Sync preserves live explicit
transaction portals as described in the
[wire transaction contract](pgwire-transaction-contract.md).

Cumulative row, memory, deadline and cancellation limits retain their current
portal authorities. Completion and error release newly owned rows, workers,
reservations and snapshot pins at their documented termination boundaries.

## Caches and specialized artifacts

Cache hits must describe the captured view, not the current live Data state.
Data epoch and collection generation are stored in the same Midge Data
transaction as their mutations. Their values must be read from the captured
transaction when used as snapshot cache identity. Existing schema,
authorization, parameter and setting cache checks remain required.

A specialized artifact or cached representation is usable only when its
existing validity fence is compatible with that view. Otherwise use a
controlled existing fallback over the same captured Data rows, or preserve
the existing explicit unsupported/error boundary. This selects no new
fulltext, vector, graph, hybrid or column codec support. Current artifact
metadata cannot silently select newer rows during fallback.

## Resource and ownership contract

Readonly owners are shared privately through reference-counted ownership;
Midge owns MVCC version retention and snapshot pins. Cassie admits its own
owner metadata and retained staged-overlay backing before construction or
capture and keeps their reservations for the actual owning lifetime. Worker
clones share the original owner rather than deep-copying session state.
Existing row, cursor, artifact, output and worker allocation admission laws
remain required. Sharing a transaction does not excuse fresh output backing
from admission or allow cancellation to release charge before live buffers.

## Finite qualification matrix

| Obligation | Required oracle | Current disposition |
| --- | --- | --- |
| TX-01 through TX-06 and TX-08 through TX-09 | Reuse mapped mutation, commit-gate, recovery, settings and savepoint evidence; add only uncovered controls | Historical evidence; current implementation replay pending |
| TX-07 | Captured whole-session COW overlay survives subsequent session writes and cursor resume | Per-cursor evidence exists; statement-wide owner pending |
| TX-10 joined sources | Commit barrier between empty, partial and multirow source reads returns only the captured version | Pending genuine RED/GREEN |
| CTE, subquery and workers | Repeated reads reuse the same transaction and captured overlay | Pending genuine RED/GREEN |
| Index and column paths | Candidate IDs, rows, metadata, generation and controlled fallback use one view | Pending path-specific barriers |
| Specialized paths and caches | Existing artifact fence or compatible cache identity; fallback stays on captured rows | Pending source-backed path qualification |
| Portals | First-execution membership, values, order and overlay persist across resumes; termination releases owners | Existing witnesses to reuse; sharing acceptance pending |
| Resource failures | Admission denial and cancellation preserve owner/row lifetime; cleanup releases owned charge | Pending focused controls |

Qualification must retain exact source, inputs, results and command provenance.
A passing selected path does not promote another path. Keep #763 open while
any selected acceptance is pending, failed or blocked. Complete repository
validation, independent adversarial review, exact-head hosted checks and
merged readback remain required before closure.
