# Extension compatibility and catalog prerequisites

Issues #817 and #843 select a finite compatibility surface for pgcrypto 1.4
and vector 0.8.7, not PostgreSQL extension ABI or arbitrary module loading.
The current source has neither extension lifecycle state nor the pgcrypto
`digest` routine. The selected identities must not be advertised as available
or installed until their create and routine contracts are actually available.

PostgreSQL 18.6 describes `pg_extension` with `oid` (oid), `extname` (name),
`extowner` (oid), `extnamespace` (oid), `extrelocatable` (boolean),
`extversion` (text), `extconfig` (oid[]) and `extcondition` (text[]).
`pg_available_extensions` has `name` (name), `default_version` (text),
`installed_version` (text), and `comment` (text). A reference install showed
pgcrypto version 1.4, relocatable in its chosen schema, with NULL extconfig and
extcondition. It supplies strict immutable `digest(text,text)` and
`digest(bytea,text)` overloads returning bytea; `digest('abc','sha256')`
returned `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

Further oracle observations: duplicate CREATE returns `42710`, and IF NOT
EXISTS skips even with a nonexistent requested schema; an unavailable version
update and an unknown digest algorithm return `22023`; relocation changes the
routine schema. The oracle transaction was rolled back and its server stopped.
The local oracle had no vector extension, so its metadata is not inferred from
that run. All Cassie tests authored for this work are unexecuted.

The first catalog-only increment adds these two exact empty relation descriptors
while no selected extension is installable. Empty available rows describe the
current implementation, not completion of #843. Builtin relation OIDs already
exist in the pinned catalog identity table. Existing `pg_namespace` supplies
namespace identities; `pg_proc` currently exposes only builtin OID/name/result
type and `pg_depend` only selected sequence ownership, so truthful digest
overload and extension-member rows require explicit later work.

Jev selected a versioned per-database owning-family representation and truthful
catalog descriptors (`target/assembly-review/extension-compatibility-contract-*`).
No lifecycle publication is activated by that decision. A complete implementation
must prove collision-safe persistent extension and routine identities; atomic
install/drop/relocation plus dependency updates; existing #833 transaction and
savepoint snapshot/delta handling; same-database storage ownership; recovery,
backup, old-reader behavior and raw native writer interactions. Published
Midge 0.5.2 remains the direct storage layer; no engine edit, second layer,
unpublished range assertion, or native extension ABI is assumed.

The lifecycle acceptance draft covers two-session commit visibility, digest
and NULL behavior, savepoint rollback of relocation, selected available
versions, stable extension identity, two distinct digest member identities,
member removal on drop, bytea overload descriptors and unselected algorithms.
These tests are unexecuted; they do not select a
durable encoding or prove the remaining contract. The existing unsupported
pgcrypto test must be updated only when installation really becomes available.

The existing `allocate_object_id` counter is in the schema family, and a
transactional database-family scope leaves other family operations raw. ID
reservation can therefore commit independently and leave unused IDs on abort;
that is not atomic publication of extension state and its dependencies.
`normal_oid` checks conversion into the PostgreSQL user range. The pinned
`pg_proc` table currently has no built-in OID at or above 16384, but that
observation alone does not prove future/user routine collision freedom. The
extension record and selected routine/dependency changes need their own
same-database atomicity and identity proof, including ambiguous outcomes.
An older live reader or writer that does not understand a new extension record
can omit its namespace dependency; a startup-only layout marker does not fence
that already-running process. Compatibility activation remains unselected until
the supported reader/writer boundary is proven.

For the fixed pgcrypto/vector names, participating Cassie schema operations
can in principle assert each exact extension key plus the exact namespace key;
an arbitrary range assertion is not necessary for those two known names.
However, the current `AdapterTransaction::assert_value` forwards only to the
inner native transaction. A staged transaction drops that inner transaction,
and `commit_storage_overlay` reasserts only keys recorded as changes, so a
pure staged read/absence assertion is not retained. A separate #833 assertion
owner with statement/savepoint rollback and final native publication is a
prerequisite; this document does not claim it has been implemented.
