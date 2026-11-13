# v0.3.0 object identifier contract (#818)

This document defines Cassie's finite `regclass`, `regnamespace`, `regtype`,
and `regprocedure` conversion and lookup contract (established for v0.2.0 and
extended for user routines and types in v0.3.0). It is limited to
the built-in identities and real catalog objects selected below; PostgreSQL
syntax adjacent to these cases does not imply broader support.

## Prerequisite and support boundary

The live [#817 issue](https://github.com/cntryl/cassie/issues/817) records the
selected v0.2 catalog fields on 2026-11-11. Its field-selection prerequisite
for #818 is satisfied: `pg_namespace` selects `oid` and `nspname`; `pg_class`
selects relation identity and namespace fields; and `pg_type` selects built-in
type identity. The earlier [v0.2 catalog source map](catalog-v020-inventory-2026-11-06.md)
records where those experimental fields are implemented. #817 remains open for
its other catalog semantics; this contract does not claim that issue complete.
#796 is closed.

`pg_proc` contains the pinned built-in routine identity rows used by
`regprocedure`; it is not a general user-routine catalog. User-defined type
identity and type rows belong to #828. Enum identities remain deferred to
#827/v0.4.0. No broad `pg_proc`, dependency, sequence-catalog, partition,
enum, or custom-type introspection is selected by this contract. `regclass`
may resolve an existing sequence as a relation, without adding sequence
catalog or sequence-operation support.

## Supported behavior

| Type and operation | Finite behavior | Missing-object behavior |
| --- | --- | --- |
| `regclass` casts and `to_regclass(text)` | Resolve real relation identities, including supported tables, views, materialized views, sequences, and indexes. Unqualified names search implicit `pg_catalog` followed by the session `search_path`; qualified names honor quoted identifiers. Stored relation identity is scoped to the active database. Text output qualifies a relation when the search path would otherwise identify a different object. | Cast input for a missing relation reports an undefined-relation error. `to_regclass` returns SQL `NULL`. |
| `regnamespace` casts and `to_regnamespace(text)` | Resolve a single schema name to built-in or stored namespace identity in the active database. Quoted schema identifiers are parsed as identifiers; a namespace from another database is not returned. | Cast input for a missing schema reports an undefined-schema error. `to_regnamespace` returns SQL `NULL`. |
| `regtype` casts and `to_regtype(text)` | Resolve one built-in type name, optionally qualified by `pg_catalog`, from the pinned type registry. SQL aliases and quoted identifiers are supported. Typmods do not change the base type OID. Array suffixes `[]` and repeated `[]` or `[integer-constant]` dimensions map to the registered array type OID; comments are treated as whitespace. | A valid but unknown or non-`pg_catalog` type returns `NULL` from `to_regtype`; a cast reports an undefined-type error. Invalid array suffix syntax, out-of-range bounds, and non-SQL whitespace in a bound report SQLSTATE `42601`, including through `to_regtype`. User composite types, table row types and domains resolve through the search path with their allocator-backed OIDs (#828). |
| `regprocedure` casts and `to_regprocedure(text)` | Resolve a pinned built-in routine by exact argument type OIDs. The name may be unqualified or qualified by `pg_catalog`; identifier quoting is honored. Argument types must resolve in the built-in type registry. Overloads of built-in routines select distinct routine identities by signature, and output retains the selected routine OID. | A well-formed missing overload or malformed helper input returns `NULL` from `to_regprocedure`; a cast reports an undefined-function or input-syntax error. User-defined functions and procedures resolve by exact argument types (built-in and composite types) through the search path; a bare `regproc` name that denotes several routines reports `42725`. Each routine's OID is the identity allocated from the common counter when it was created; it survives restart and redefinition, a dropped and recreated routine gets a new one, and a routine stored before identities existed publishes none. User routine names match case-insensitively, because the catalog keys them that way, so quoted names that differ only in case are not distinguished. General `pg_proc` rows for user routines are outside this contract. |

The aliases retain their PostgreSQL 18 type OIDs (`regclass` 2205,
`regnamespace` 4089, `regtype` 2206, and `regprocedure` 2202). The selected
`to_reg*` helpers each accept one `TEXT` argument (OID 25) and return their
matching alias type. The focused protocol test asserts untyped bound-name
parameter inference, resolved OIDs, and descriptors for both casts and lookup
expressions. Integer/OID conversion continues to use the existing `oid` range
contract.

## Deterministic exclusions

- This contract covers only the four named aliases and their matching
  one-argument text helpers. Other `reg*` types and helpers retain their
  existing independent support contracts.
- `regtype` resolves identities in the pinned built-in registry plus the
  allocator-backed OIDs of user composite types, table row types and domains
  (#828), each with its `_name`/`[]` array type. A valid unknown type yields SQL
  `NULL` through `to_regtype`; a cast reports undefined type.
- `regprocedure` resolves pinned built-in and user-defined routine identities by
  exact argument types. It does not infer a routine by name or coerce
  signature argument types. A missing helper lookup returns SQL `NULL`; a cast
  reports undefined function. Malformed helper signatures also return SQL
  `NULL`, matching PostgreSQL 18.6's `to_regprocedure` behavior.
- Lookups are privilege-aware: unqualified names skip schemas without `USAGE`
  and qualified `regclass`, `regtype`, `regproc` and `regprocedure` names in a
  schema without `USAGE` raise `42501`, as in PostgreSQL (see Visibility).
- Cassie has no routine overloads within a schema: `CREATE OR REPLACE
  FUNCTION|PROCEDURE` with a changed argument list is `0A000` and does not
  replace the routine. See [Type Contract](type-contract.md) for the related
  routine, row-type and owner-check limits.

The relevant source is `src/catalog/object_identity.rs`,
`src/catalog/object_identity/names.rs`,
`src/catalog/object_identity/user_types.rs`,
`src/catalog/user_type_resolution.rs`,
`src/catalog/object_identity/routines.rs`, and
`src/executor/filter/object_ids.rs`. Built-in type and routine identities are
read from the pinned data under `src/catalog/builtin_identity/`.

## Visibility

Unqualified `regclass`, `regtype`, `regprocedure` and `regproc` resolution
skips schemas on the `search_path` for which the authenticated role lacks
`USAGE`; `to_reg*` returns `NULL` and a cast reports the undefined-object
error until the role has `USAGE`, independent of the object's own privileges.
A qualified name in a schema without `USAGE` raises SQLSTATE `42501`
("permission denied for schema") from both the cast and `to_reg*`, even when
the object does not exist; `regnamespace` needs no `USAGE`. PostgreSQL 18.6
agrees on every one of these cases. The restriction applies to
network-authenticated non-admin roles; `root` is unrestricted. Cassie records
no schema owners other than `root`, so there is no separate owner path.
#831 is closed. Evidence:
[`identifier_visibility.rs`](../tests/catalog_security/security/object_privilege_grants/identifier_visibility.rs)
covers `regclass`, `regtype`, `regprocedure` and `regnamespace`, qualified and
unqualified, with and without `USAGE`, and the `root` bypass.

## Name resolution and spelling rules

These rules were fixed after the v0.3.0 adversarial review; PostgreSQL 18.6 is
the oracle for each. The regressions are authored and their execution is
deferred to the batch gate.

- **One search-path walk for user types.** A type name is unique within a
  schema across enum, composite, domain and table row types, so an unqualified
  name denotes the type in the first schema on the `search_path` that holds
  any user type of that name, whatever its kind. `regtype` input, `regtype`
  output and the cast binder (`x::name`, `null::name`) share that walk
  (`Catalog::resolve_user_type`). `regtype` output is schema-qualified exactly
  when that walk would find a different type, so output always round-trips.
- **Array spellings.** A user type's array type resolves from `name[]`, any
  number of `[]` or `[n]` dimensions, and its `pg_type.typname` (`_name`,
  optionally schema-qualified).
- **White space.** White space around the dot of a qualified name is
  insignificant, and the schema-`USAGE` check reads the schema by the same
  rule: `'s . t'`, `'s .t'` and `'db . s . t'` all require `USAGE` on `s`.
- **Too many dotted parts and other databases.** A relation, type or routine
  name with more than three dotted parts raises `42601`, and a three-part name
  whose database is not the current one raises `0A000`, from both the cast and
  the `to_reg*` helper. `to_regnamespace('a.b')` returns `NULL`.
- **Quoted names.** A quoted relation, schema or type name is exact. An
  unquoted type name folds to lower case and is exact, because type DDL folds
  it. An unquoted relation or schema name matches without regard to case:
  unquoted `CREATE TABLE` and `CREATE SCHEMA` keep the case they were written
  in, so `'mixs.mixt'::regclass` still finds a relation created as
  `"MixS"."MixT"` (PostgreSQL: `42P01`). That residual divergence ends only
  when relation and schema DDL fold unquoted names. Routine names stay
  case-insensitive as described above.
- **`regnamespace` output** is `quote_ident` of the schema name: a name
  holding a quote is escaped once (`"q""s"`).
- **Routine arguments.** A function or procedure may declare domain and enum
  argument types as well as built-in and composite ones; `regprocedure`
  identifies the argument by the declared type, not a domain's base type. A
  routine keeps the composite or domain types of its arguments and result
  alive: `DROP TYPE` and `DROP DOMAIN` raise `2BP01`. The same dependency for
  enum types is recorded as a pending obligation of the enum drop path
  (`Catalog::routine_dependents_on_type` supplies the dependents).
- **Schemas own their routines.** A function or procedure blocks a plain
  `DROP SCHEMA`, and `DROP SCHEMA ... CASCADE` drops it.
- **Index identity.** `CREATE INDEX` assigns the index's `pg_class.oid` from
  the common identity allocator, so it cannot collide with another object and
  survives rename and restart. An index stored without an assigned identity
  (a constraint-backing index, or one created before this change) keeps a
  name-derived OID in the upper half of the OID space, which no allocator
  identity reaches.
- **`pg_identify_object` sub-objects.** The sub-object number is
  `pg_attribute.attnum`: a dropped table column or composite attribute keeps
  its number and is identified by PostgreSQL's `........pg.dropped.N........`
  placeholder, and later columns do not shift. Arguments may be scalar
  subqueries.

Residual divergences recorded for these rules:

- A function and a procedure of the same name and argument types can coexist
  (PostgreSQL: `42723`, see [Type Contract](type-contract.md)); `regproc` and
  `regprocedure` then resolve the function and never the procedure.
- `pg_attribute.attrelid` is the relation name as text and `attrelid_oid` the
  OID ([Catalog Support](catalog-support.md)), so `attrelid = 't'::regclass`
  and `attrelid = pg_class.oid` are rejected (`42601`); compare
  `attrelid_oid` instead. Making `attrelid` an `oid` is a catalog-contract
  change that every `attrelid = 'name'` consumer depends on.

Evidence (authored, not yet run):
[`user_type_search_path.rs`](../tests/catalog_security/catalogs/types/object_identity/user_type_search_path.rs),
[`user_type_array_names.rs`](../tests/catalog_security/catalogs/types/object_identity/user_type_array_names.rs),
[`identifier_spelling.rs`](../tests/catalog_security/catalogs/types/object_identity/identifier_spelling.rs),
[`routine_user_type_arguments.rs`](../tests/catalog_security/catalogs/types/object_identity/routine_user_type_arguments.rs),
[`schema_routines.rs`](../tests/catalog_security/catalogs/types/object_identity/schema_routines.rs),
[`index_identity.rs`](../tests/catalog_security/catalogs/types/object_identity/index_identity.rs),
[`identify_dropped_columns.rs`](../tests/catalog_security/catalogs/types/object_identity/identify_dropped_columns.rs)
and the white-space cases in
[`identifier_visibility.rs`](../tests/catalog_security/security/object_privilege_grants/identifier_visibility.rs).

## Evidence

The relevant existing tests are:

- [`catalog_identity_lookups.rs`](../tests/compatibility/lifecycle/routines/catalog_identity_lookups.rs): prepared `regclass` lookup through relation DDL, bound text input, overloaded and malformed `regprocedure` lookups, nullable missing objects, namespace restart identity, and quoted routine overloads.
- [`semantics.rs`](../tests/catalog_security/catalogs/types/catalog_discovery/semantics.rs): relation search-path precedence, overload signatures and aliases, bounded built-in array names, and current-database isolation for relations and namespaces.
- [`catalog_identity_lookups.rs`](../tests/compatibility/lifecycle/casts/catalog_identity_lookups.rs): missing-object cast errors.

The #818 increment adds focused checks that malformed array type syntax stays
an error through `to_regtype`, that the selected aliases expose their expected
wire descriptors in
[`should_report_postgresql_descriptors_for_selected_reg_aliases`](../tests/compatibility/lifecycle/routines/catalog_identity_lookups.rs),
that all four lookups resolve bound text names in
[`should_resolve_text_parameters_for_selected_reg_lookups`](../tests/compatibility/lifecycle/routines/catalog_identity_lookups.rs),
and that `regnamespace` only resolves namespaces in the active database.
The executed expectations are:

- Cast and lookup-expression result descriptors are each `[2205, 4089, 2206,
  2202]` for `regclass`, `regnamespace`, `regtype`, and `regprocedure`.
- Bound text names resolve to OIDs `[1259, 2200, 23, 1397]` through the four
  selected `to_reg*` helpers.
- Missing `to_regclass`, `to_regnamespace`, `to_regtype`, and
  `to_regprocedure` objects return SQL `NULL`; the namespace test also proves
  that a missing schema and a schema present only in another database do not
  resolve in the active database.
- The four malformed array type names below return SQLSTATE `42601` through
  `to_regtype`; a valid unknown type returns SQL `NULL`.

PostgreSQL 18.6 was used as the semantic oracle for the type-name edge cases.
It returned SQLSTATE `42601` for `to_regtype` inputs `integer[1__0]`,
`integer[2147483648]`, `integer[0x80000000]`, and a non-SQL whitespace character
inside the array bound; a valid unknown type returned `NULL`. PostgreSQL's
[`to_regtype` documentation](https://www.postgresql.org/docs/18/functions-info.html)
distinguishes malformed type-name syntax from a valid type name that does not
exist. The [object identifier type documentation](https://www.postgresql.org/docs/18/datatype-oid.html)
describes name resolution through the current search path.

## Final local validation

The final source, regression tests, and this contract passed the required
ordered local gates:

| Command | Result |
| --- | --- |
| `cargo build --locked` | Passed. |
| `cargo test --locked` | Passed with exit status 0; all workspace unit, integration, and doc-test targets completed without failure. The library target ran 2,879 tests. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `cntryl-tools validate-tests -f tests/compatibility/lifecycle/routines/catalog_identity_lookups.rs` | Passed, 13/13 compliant. |
| `cntryl-tools validate-tests -f tests/catalog_security/catalogs/types/catalog_discovery/semantics.rs` | Passed, 28/28 compliant. |
| `cntryl-tools validate-tests -f src/catalog/object_identity/names.rs` | Passed, 8/8 compliant. |
| `cntryl-tools validate-tests -f src/sql/functions/inputs.rs` | Passed, 5/5 compliant. |

One preliminary full-suite run had an unrelated Midge write conflict in
`projection_concurrency::should_serialize_concurrent_materialized_projection_refreshes`.
That test passed when rerun alone, and the final full locked test run passed.
The bound-parameter regression also exposed SQLSTATE `42P18` before the
selected functions received explicit `TEXT` input signatures; the final full
test run verifies the corrected behavior. The malformed-type regression was
authored before its parser change; no unobserved baseline failure is claimed.

The current-release contract does not require migration or compatibility
between unreleased development builds.
