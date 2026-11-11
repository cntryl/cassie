# Built-in extension compatibility qualification

## Scope and dependency gate

This record covers the bounded v0.2.0 slice in [#843](https://github.com/cntryl/cassie/issues/843): pgcrypto 1.4 lifecycle and SHA-256 digest compatibility, plus vector 0.8.7 extension identity. Vector SQL/type/retrieval semantics remain in [#844](https://github.com/cntryl/cassie/issues/844). Midge remains Cassie's only storage layer; no native extension libraries or arbitrary extension ABI are loaded.

The current #843 body names `docs/catalog-support.md` as its prerequisite. That document and #817's current acceptance select exactly these #843 fields: `pg_extension(oid, extname, extowner, extnamespace, extrelocatable, extversion)`, `pg_available_extensions(name, default_version, installed_version)`, and `pg_proc(oid, proname, prorettype, pronamespace)` for the two pgcrypto digest identities while installed. This satisfies the catalog contract needed by #843. The older #843 triage note saying “after #817” predates this finite selection. #817 remains open for its broader runtime truthfulness and lifecycle/query evidence; this record does not claim that issue complete.

A focused Jev choice selected **A: proceed with bounded #843 work** (confidence 0.82; A 0.88, B 0.10, none 0.02). This was a scope judgment only. The live issue bodies, comments, and selected Catalog Support contract were checked separately. The request and typed response are retained under ignored `target/jev/issue-843-817-dependency-gate-{request,response}.json` on the implementation worktree.

At base `22a8324dc9110d1b0b56dbc3eaa8ee8f0498caef`, source has descriptors for `pg_extension` and `pg_available_extensions`, but their virtual-view dispatcher returns empty rows; no pgcrypto registration or SQL `digest` implementation exists. Authored acceptance tests are not implementation evidence.

## Selected digest-view dependency contract

PostgreSQL 18.6 was probed in isolated databases with its installed pgcrypto 1.3 package. In `issue843_default_schema_20261111`, `SET search_path TO crypto_schema, public` made `current_schema()` report `crypto_schema`; `CREATE EXTENSION pgcrypto` without `SCHEMA` installed into that schema. In `issue843_semantics_20261111`, a view created against `public.digest` remained dependent after `ALTER EXTENSION pgcrypto SET SCHEMA crypto_schema`: `DROP EXTENSION pgcrypto RESTRICT` was caught as SQLSTATE `2BP01`, `DROP EXTENSION pgcrypto CASCADE` removed the view, and the extension/procedure namespace read back as `crypto_schema`. Therefore the default installation schema follows the current schema, and dependency detection cannot rely on comparing stored SQL qualifiers with the extension's current schema.

```sql
CREATE SCHEMA crypto_schema;
CREATE EXTENSION pgcrypto WITH SCHEMA public VERSION '1.3';
CREATE VIEW digest_dep AS
  SELECT encode(public.digest('abc', 'sha256'), 'hex') AS hash;
ALTER EXTENSION pgcrypto SET SCHEMA crypto_schema;
-- RESTRICT raises 2BP01; CASCADE drops digest_dep.
```

The selected #843 implementation records the exact installed extension object identity on a stored view when its bound query resolves a pgcrypto `digest` call. This metadata follows the view through storage and transaction snapshots. `DROP EXTENSION` uses the recorded identity for RESTRICT/CASCADE after schema relocation. This is the selected pgcrypto workflow only; it does not add broad `pg_depend` support. No compatibility or migration behavior between unreleased builds is selected.

A focused Jev choice selected **persist the resolved extension identity** (confidence 1.0; probability 1.0) over current-schema text matching, treating every `digest` string as dependent, or leaving the relocation gap. The question and typed response are retained under ignored `target/jev/issue-843-runtime-review-{request,response}.json`. This judgment is a scope lead, checked against the source model and PostgreSQL oracle; it is not implementation or correctness evidence.

## Implementation and validation evidence

The source freeze now includes durable selected extension metadata, active-database extension/procedure catalog rows, the bounded extension DDL parser and executor, and installed-only pgcrypto digest binding/runtime. An omitted `SCHEMA` follows the session's current schema. Storage writes and hydration accept only pgcrypto 1.4/vector 0.8.7 records. View metadata records exact selected extension object IDs for resolved digest calls so RESTRICT/CASCADE remain tied to the installed routine identity across schema relocation.

Focused `should_` tests cover exact extension/procedure descriptors, available/installed rows, rejected profiles, lifecycle and SQLSTATEs, current-schema installation, transactional install/drop rollback, digest BYTEA/NULL/SHA-256 behavior, schema relocation, dependency enforcement before and after relocation, vector identity only, and local restart of extension/view dependency state. The selected catalog field contract and the #817 dependency distinction are also documented in `docs/catalog-support.md`.

A focused Jev source review weakly selected `ready_for_ordered_gates` (probability 0.53; `confirmed_in_scope_gap` 0.43). The response is retained under ignored `target/jev/issue-843-implementation-review-{request,response}.json`. Because the gap probability remained material, the source was checked against focused PostgreSQL 18.6 probes for current-schema installation and schema-relocation dependencies; both expected behaviors are covered by regressions and implementation. These probes and Jev are not evidence that Cassie's tests pass.

The ordered local gate passed on the frozen slice on 2026-11-11:

1. `cargo build --locked --bin cassie` — passed.
2. `cargo test --locked` — passed, including the full unit, integration, and documentation-test targets.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` — passed.
4. `cargo fmt --all -- --check` — passed.
5. `cntryl-tools validate-tests -f <path>` — passed for all eight touched test files. The DDL mount file contains no tests itself; its validator returned success.

The local gates and pinned PostgreSQL 18.6 probes do not establish hosted PR checks, merge, or issue closure; those remain coordinator-owned close-out steps.
