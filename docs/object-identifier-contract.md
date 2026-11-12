# v0.2 object identifier contract (#818)

This document defines Cassie's finite `regclass`, `regnamespace`, `regtype`,
and `regprocedure` conversion and lookup contract for v0.2.0. It is limited to
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
| `regtype` casts and `to_regtype(text)` | Resolve one built-in type name, optionally qualified by `pg_catalog`, from the pinned type registry. SQL aliases and quoted identifiers are supported. Typmods do not change the base type OID. Array suffixes `[]` and repeated `[]` or `[integer-constant]` dimensions map to the registered array type OID; comments are treated as whitespace. | A valid but unknown or non-`pg_catalog` type returns `NULL` from `to_regtype`; a cast reports an undefined-type error. Invalid array suffix syntax, out-of-range bounds, and non-SQL whitespace in a bound report SQLSTATE `42601`, including through `to_regtype`. User-defined type identities belong to #828. |
| `regprocedure` casts and `to_regprocedure(text)` | Resolve a pinned built-in routine by exact argument type OIDs. The name may be unqualified or qualified by `pg_catalog`; identifier quoting is honored. Argument types must resolve in the built-in type registry. Overloads select distinct routine identities by signature, and output retains the selected routine OID. | A well-formed missing overload, unsupported routine name, or malformed helper input returns `NULL` from `to_regprocedure`; a cast reports an undefined-function or input-syntax error. User-defined routine identities and general `pg_proc` rows are outside this contract. |

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
- `regtype` only resolves identities in the pinned built-in registry. A valid
  unknown custom type yields SQL `NULL` through `to_regtype`; a cast reports
  undefined type until #828 selects and implements custom type identity.
- `regprocedure` only resolves pinned built-in routine identities. It does not
  infer a routine by name, coerce signature argument types, or look up a
  user-defined routine. A missing helper lookup returns SQL `NULL`; a cast
  reports undefined function. Malformed helper signatures also return SQL
  `NULL`, matching PostgreSQL 18.6's `to_regprocedure` behavior.
- No privilege-aware search-path visibility is claimed. It is the remaining
  #831 dependency described below.

The relevant source is `src/catalog/object_identity.rs`,
`src/catalog/object_identity/names.rs`,
`src/catalog/object_identity/routines.rs`, and
`src/executor/filter/object_ids.rs`. Built-in type and routine identities are
read from the pinned data under `src/catalog/builtin_identity/`.

## Visibility dependency

Privilege-aware search-path visibility remains blocked by #831 and is not
claimed as implemented here. Current `BindingContext` carries database and
`search_path` but no authenticated principal. `RoleMeta` stores database
grants only; `pg_namespace.nspacl` is currently an empty text value, and
`has_schema_privilege` returns `true` without checking grants. The relation
search order therefore does not filter schemas by `USAGE`. Once #831 supplies
the named schema-usage contract, its integration must add regressions proving
that inaccessible schemas are skipped during unqualified resolution and that
qualified lookups follow that contract. Until then this acceptance item is
open, and this issue must not be marked complete.

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
