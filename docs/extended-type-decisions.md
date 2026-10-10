# PostgreSQL 18 type and durable-format decisions

[#793](https://github.com/cntryl/cassie/issues/793) owns the contracts needed by
the [selected PostgreSQL 18.6 delivery program](postgresql18-delivery.md).
Full PostgreSQL semantics are the final target. Earlier 38-digit decimals,
one-dimensional arrays and fixed-offset time selections remain intermediate
slices. The [current finite type contract](type-contract.md) describes existing
runtime support; these new records do not promote that support.

## Selected direction and remaining acceptance

The user selected `cassie-midge-layout-v3` with verified export/recreate/reimport
on 2026-10-08, permitting breaking changes while preserving the original store
and executable for rollback. On 2026-10-10 the user explicitly approved the
concrete [common CRB3/type identity/codec contract](postgresql18-v3-contract.md).
Its exact approved proposal is retained in
[the approval snapshot](../tests/corpus/postgresql18/v3-approved-proposal.txt),
SHA256 `26c43b1718f94160be6031d9bfc32ae301575df63ab71367498394e77f04ecc6`.
The snapshot's historical proposal wording records what was reviewed; the
approval above supersedes that historical status. Do not ask for this common
direction again.

The user subsequently selected the concrete
[catalog/export schemas](postgresql18-catalog-export-proposal.md) on 2026-10-10
by directing work to proceed after the remaining decision was identified.
The JSON record schemas, database-qualified Midge keys, global named-object
allocator extension and staged import publication are now selected. The exact
[approved catalog/export proposal](../tests/corpus/postgresql18/v3-catalog-export-approved-proposal.txt)
has SHA256 `c2bd35c33658e5bdccc46809226a4c46d2b6bb89999fa219124ad9ca49b8b625`.
Its historical pending status is superseded; do not request this decision again.
Approval does not implement or qualify these bytes. Complete each affected
record and its recorded prerequisites before dependent runtime.

## Contract records

- [Twenty-two eight-field type-family records](postgresql18-type-families.md)
  select the exact PostgreSQL value, syntax, wire, durable-codec, lifecycle,
  resource, evidence and implementation-owner laws.
- [Shared type resolution and resource rules](postgresql18-type-resolution-resources.md)
  retain the Rust pipeline and one type/catalog authority, exact coercion
  contexts and overload resolution, encoding semantics and checked admission.
- [Immutable built-in metadata and source inputs](../tests/corpus/postgresql18/type-contract-inputs.json)
  enumerate 193 built-in types, PostgreSQL metadata, codec classifications,
  source hashes and bound/flag constants. Every runtime qualification is pending.
- [Coercion/operator catalog inputs](../tests/corpus/postgresql18/type-resolution-inputs.json)
  retain the exact source rows, including 235 casts, with hashed catalog headers
  defining omitted defaults. [Reference build metadata](../tests/corpus/postgresql18/type-reference-build.json)
  records the local reference configuration; full provider/deployment provenance
  remains final qualification work.
- [Reference type observations](../tests/corpus/postgresql18/type-oracles.json),
  [encoding/float observations](../tests/corpus/postgresql18/type-encoding-oracles.json)
  and [composite/range descriptor observations](../tests/corpus/postgresql18/type-descriptor-oracles.json)
  capture the pinned reference only. Expected errors are observations, not passes.
- [Durable acceptance vectors](../tests/corpus/postgresql18/v3-contract-vectors.json)
  define literal scalar, private-container and malformed controls for future
  encoders/decoders. They are not implemented codec or restart evidence.

Each family records these eight fields before its runtime increment: accepted
syntax; value/NULL/equality/order/hash/coercion laws; names/OIDs/typmods/wire
codecs and parameter/result descriptors; versioned durable representation and
upgrade behavior; catalog/dependency/cache lifecycle; bounds/accounting and
cancellation; independent oracle/durable/cross-path/restart witnesses; and
approval provenance with exact implementation owners.

PostgreSQL source routines and the hashed catalog signatures are normative;
summary prose does not replace their edge cases. Named reference OIDs belong
to the isolated reference catalog. Compare their identity graphs through the
approved mapping while retaining exact built-in OIDs and independently testing
Cassie's named identity stability.

## Persistent transition and recovery

V2 and v3 writers reject the other layout before mutation. There is no selected
mixed-version reader or in-place upgrade. Export the quiesced original with its
actual type authority: historical NUMERIC/FLOAT and JSONB/JSON aliases retain
their stored interpretation. Preserve SQL NULL, absent fields, multiplicity,
identities/dependencies, privileges, sequence state and allocator reservations.
Rebuild derived sidecars from imported authoritative values.

Import into a fresh separate store, reject unsupported/malformed state before
publication, verify exact values/catalog meaning and restart against the source,
then reconnect clients with fresh prepared statements. Keep the original pair
intact. Rollback uses that pair; post-switch writes require a separately verified
reverse-export path. A layout marker or export command is insufficient evidence.

## Prerequisites and acceptance status

#904 implements the shared registry and extracts oversized catalog modules before
extending them. #833 owns transactional catalog publication; #905 owns ranges
and multiranges independently of #828 composites/records/domains. Primitive
range slices precede #824 constraints, then #826 tables and #828 integration.
Record each staged prerequisite's completed commit and tests before dependent
work. Later SQL-object options, prepared-transaction and large-object schemas
remain explicit #910–#916 owner gates rather than invented codec assumptions.

The existing vector profile remains supported according to its current contract;
#844's extended wire/extension identity record still needs its exact pinned
extension matrix. Internal/pseudo-types retain real context/error behavior;
an I/O catalog entry does not invent an externally constructible stored value.

#793 remains open: remaining staged owner records, runtime qualification and
actual upgrade verification are incomplete. The common and catalog/export
decisions are selected. #848 owns complete differential
qualification, #849 accepts resource/restart evidence, and #791 closes last.
Failed, skipped, unavailable, pending and unsupported in-scope cells keep the
program open. The full ordered local gate, adversarial probes, exact-head hosted
checks and checked squash/readback remain required for each delivery bundle.
