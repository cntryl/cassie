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
current-format backup/restart/restore behavior and raw native writer interactions. Published
Midge 0.5.2 remains the direct storage layer; no engine edit, second layer,
unpublished range assertion, or native extension ABI is assumed.
