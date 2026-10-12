# PostgreSQL 18 Differential Corpus

[#848](https://github.com/cntryl/cassie/issues/848) owns complete qualification.
This stage-0 bootstrap supplies executable comparisons and a pending inventory;
it does not complete that gate or claim dialect support. The
[delivery contract](postgresql18-delivery.md) defines scope and closure.

## Immutable inputs

[Reference manifest](../tests/corpus/postgresql18/reference.json) pins PostgreSQL
18.6 source archive SHA-256 and the local Darwin arm64 postgres/initdb executable
hashes. The runner verifies the binaries before initializing an isolated
reference store, starts its own local socket-only server, checks server_version_num
and shuts it down after collection. A different executable fails admission;
Linux/container qualification needs a separately recorded immutable artifact.
Version strings alone are insufficient. Runtime library hashes are retained in
receipts; complete deployment/build provenance remains a final qualification
obligation.

[Upstream inventory](../tests/corpus/postgresql18/inventory.json) is generated
from the checksum-verified official source archive: SQL command synopses,
pg_type.dat including generated array identities, pg_proc.dat signatures,
catalog and PL/pgSQL anchors, explicit query contexts and regression/isolation
input hashes. These are coverage-review inputs. Every entry begins pending;
owners must classify individual forms, expand syntax/overloads and assign
execution-path/context cells. Internal function and physical-server entries
require explicit classification under the recorded architectural exceptions;
their implementation language does not by itself exclude a built-in SQL function.

Reproduce the inventory using the verified archive without extracting files:

```sh
python3 -m tools.pg18_parity.inventory \
  --archive target/parity-evidence/postgresql-18.6.tar.bz2 \
  --manifest tests/corpus/postgresql18/reference.json \
  --output target/parity-evidence/inventory-regenerated.json
```

## Bootstrap execution

[Bootstrap cells](../tests/corpus/postgresql18/bootstrap.json) exercise exact
BIGINT values, SQL NULL versus text NULL, extrema, binary/NULL parameters and
SQLSTATE across applicable simple text, extended text and extended binary
paths. The development-only libpq adapter preserves raw value bytes, result
column names/OIDs/typmods/lengths/formats, prepared descriptors/parameter OIDs,
SQLSTATE and command tags. Unordered rows compare as bags with multiplicity;
declared ordering compares sequences. Multiple simple-query results are retained.
COPY streaming and further lifecycle adapters remain pending and cannot pass by
being unavailable.

[Boolean cells](../tests/corpus/postgresql18/boolean.json) add 278 forms over
556 extended text/binary paths to the default runner. The #928 subset comprises
552 paths: all 256 binary receive bytes, NULL/width controls, inferred/repeated
parameters, empty results, text inputs and selected DML/index/transaction-overlay
queries. Four ordinary required cells retain the confirmed #904 projected-before-
inferred parameter discrepancy; they remain failed obligations. No failure is
filtered from the runner or counted as supported. The separate Rust
`boolean_binary` owner checks named statement reuse, Sync recovery, legacy-array
normalization and savepoint/restart behavior. Legacy bool-array OID 34016 is
distinct from PostgreSQL 1000; those adapted witnesses cannot establish common
array descriptor parity. Failed-transaction SQLSTATE qualification remains
under #780/#848. The recorded 25P02 versus 22000 discrepancy is corrected in
source by the #819 typed transaction errors; corpus requalification is pending.

Column descriptors retain authoritative raw name bytes as `name_hex`. `name`
is a UTF-8 display value, or null when those bytes cannot be displayed as UTF-8;
it never replaces the byte identity. This preserves legal SQL_ASCII and other
encoding-dependent names without replacement-character collisions. Exact
comparison includes both fields, including prepared descriptors.

Run against a separately started isolated Cassie instance. Store the connection
string in CASSIE_PARITY_DSN; receipts do not include credentials or the DSN.
The full Cassie commit argument is caller-asserted until linked to independent
instance/build/deployment evidence. A receipt cannot certify an arbitrary
endpoint's revision from the argument alone.

```sh
python3 -m tools.pg18_parity.run \
  --reference-bin-dir /opt/homebrew/opt/postgresql@18/bin \
  --libpq /opt/homebrew/opt/postgresql@18/lib/postgresql/libpq.dylib \
  --cassie-revision <full-40-character-commit> \
  --output target/p18-run
```

Use a fresh, short output path; reference socket paths have operating-system
length limits. Each invocation retains its reference logs/store and receipt
under ignored target output. Code 1 means a differential mismatch; code 2 means
unavailable or incomplete qualification, including a passing bootstrap with
pending inventory. This bootstrap deliberately has no complete-qualification
success exit. Failed, skipped, unavailable, pending or unsupported in-scope
cells keep #848 and the milestone open.

The comparator's independent checks run with:

```sh
python3 tests/pg18_parity/test_comparison.py
python3 tests/pg18_parity/test_wire.py
python3 tests/pg18_parity/test_cancellation.py
python3 tests/pg18_parity/test_column_names.py
```

The wire tests use the pinned local reference and fail when it is unavailable;
they do not silently skip. Repository Rust test-policy validation does not
discover Python tests, so these explicit commands are required in addition to
the full ordered repository gate. These adapters are development tooling and
do not add a storage layer or replace Cassie's Rust pipeline.

## Type-contract reference inputs

The [type decision index](extended-type-decisions.md) links 193 built-in metadata
records, 22 eight-field family records, exact coercion/operator source inputs
and source constants/hashes. Type observations capture 31 cases over 93
simple-text/extended-text/extended-binary paths, including expected errors.
Additional encoding/float and composite/range observations refine those records.
These are **reference-only** captures; no Cassie runtime cell passes because its
reference observation exists. Named fixture OIDs belong to the isolated
reference catalog and need the approved identity mapping for later comparisons.

The literal v3 vectors specify future scalar/container/golden/negative codec
acceptance. They do not certify an implemented encoder, decoder, restart or
upgrade. The common v3 direction and separately reviewed exact catalog/export
schemas are approved; their historical proposal snapshots are retained. The existing runner does not automatically
replay these richer setup/type/descriptor records. Owners must add its adapters
and concrete path/context cells; unsupported adapters remain pending under #848.

## Growth and final gate

`scalar-receive.json` adds 73 forms over 146 extended text/binary paths.
The #930 primitive error bundle owns 102 paths; #814 retains twelve temporal
endpoint/presentation paths and #807 retains 32 standard-array identity paths.
Those broader cells use PostgreSQL identities and complete comparisons. They
are never converted to passes by the separate, explicitly adapted legacy-array
SQLSTATE probes. Every default run includes the complete file, including known
remaining type obligations, alongside all prior bootstrap/RETURNING/Boolean cells.

Every bundle adds its exact forms/signatures, independent semantic assertions,
path/context matrix and immutable receipts. Include scalar/typed/indexed/overlay,
simple/extended text/binary, concurrency/privilege/locking/notification,
cancellation/rollback/savepoints, restart/schema publication, prepared-transaction
and large-object state, verified upgrade round trips, pinned generated clients
and pg_dump/restore. Clocks, randomness and physical plans require explicit
semantic assertions rather than byte equality or discarded differences.

Final qualification must join required inventory/path cells to receipts with
no missing, duplicate, unowned or nonpassing cells, verify instance revisions,
and finish every owner/prerequisite and #849 resource/restart acceptance.
The bootstrap's pending inventory count and incomplete classification cannot
be promoted by passing the sample cases.
