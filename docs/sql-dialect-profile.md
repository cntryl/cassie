# PostgreSQL 18 SQL dialect profile

This is the selected delivery contract for [#792](https://github.com/cntryl/cassie/issues/792)
under [#791](https://github.com/cntryl/cassie/issues/791), refreshed from main
`769d4db8fb7e60f0a29fade73137433a20e5a817` on 2026-10-10.
Complete **PostgreSQL 18.6 core SQL and full PL/pgSQL compatibility** is the
top-priority target. Publication records intent and dependencies; it does not
establish implementation, conformance or readiness. [Feature Support](feature-support.md)
and [Production Readiness](production-readiness.md) remain the current-status authorities.

## Selected scope

Keep the Rust parser/binder/planner/executor pipeline, direct Midge storage,
single-node execution and pgwire as the primary interface; REST is secondary
and administrative. Centralize lexical handling, precedence, type resolution
and catalog lookup in focused modules. Extend the public AST and value types
through the owning modules rather than introducing another storage abstraction.

The final boundary includes every PostgreSQL core SQL form and full PL/pgSQL,
including XML, geometry, money, pg_lsn, ranges/multiranges, TABLESAMPLE,
exclusion/temporal constraints, inheritance, SQL-defined aggregates/casts/
conversions/operators/operator families, rewrite rules, event triggers,
extended statistics, local two-phase transactions, large objects and real
Midge-backed SQL maintenance. Earlier 38-digit decimals, one-dimensional
arrays, fixed-offset temporal behavior and procedural subsets are **intermediate
delivery slices**, never final acceptance limits.

Only these architectural exceptions may be excluded: native C/plugin ABIs,
other procedural-language runtimes, foreign storage/federation, replication and
PostgreSQL-specific physical server management such as tablespaces and
ALTER SYSTEM. Native plugin access-method/language handlers and provider-specific
server management fall under those exceptions; equivalent SQL-level behavior
within direct Midge remains in scope. Unsupported in-scope forms remain open
corpus obligations; accepting syntax or returning empty catalogs does not
complete their semantics. The former closed-scope restriction is superseded.

## Shared contracts and delivery

[PostgreSQL 18 Delivery](postgresql18-delivery.md) owns the complete owner list,
stages, dependencies, exceptions and closure conditions. [Gap Analysis](postgres-sql-parity-gap-analysis.md)
maps the prior survey to these owners. Reuse PRs #877/#878/#885, revise their
scope as necessary and complete their missing validation; their historical
contents do not complete the expanded contracts.

- [#904](https://github.com/cntryl/cassie/issues/904) introduces one shared
  type/OID/typmod/coercion registry for binding, execution, codecs, catalogs and
  prepared statements. Built-in wire identities follow PostgreSQL; named types
  receive stable catalog identities. Every type owner qualifies interaction with
  existing expressions and operators.
- [#793](https://github.com/cntryl/cassie/issues/793) records value domains,
  text/binary framing, catalogs, resource bounds and durable formats before
  dependent runtime. The selected transition is `cassie-midge-layout-v3` with
  verified export/recreate/reimport into a separate store and preservation of
  the original store/executable for rollback. Existing v2 values must retain
  their original interpretation; a layout marker alone is not a migration.
- Extract oversized catalog modules before extending them. Keep source,
  benchmark and support modules below 1,000 lines and flat integration suites
  within the documented limit.
- Define coherent bundles before coding, integrate one PR at a time and use one
  heavy local validation lane. Record completed prerequisite commits and tests;
  distinguish implementation prerequisites from final integration obligations.

## Semantic reference and qualification

Pin PostgreSQL **18.6** to an immutable artifact and record its exact identity
with every receipt. The [bootstrap harness](postgresql18-corpus.md) begins in
stage 0; grow its corpus with every bundle. Upstream [SQL commands](https://www.postgresql.org/docs/18/sql-commands.html),
[PL/pgSQL](https://www.postgresql.org/docs/18/plpgsql.html) and
[regression tests](https://www.postgresql.org/docs/18/regress.html) are inventory
inputs. Bootstrap samples do not establish complete coverage.

Acceptance compares values, multiplicity, declared ordering, NULL behavior,
coercion, descriptors/OIDs/typmods, SQLSTATE, command tags and parameters across
all applicable scalar, typed, indexed, overlay, simple/extended pgwire and
text/binary paths. Include concurrent visibility/isolation/privileges, lock
release, notifications, cancellation, rollback/savepoints, restart/schema
publication, prepared-transaction and large-object recovery, verified upgrades,
pinned clients/generated ORM SQL and pg_dump/restore. Explicit semantic
assertions govern clocks, randomness and physical-plan differences.

Every runtime increment uses focused should_ tests with Arrange/Act/Assert and
red → green → refactor evidence. Async tests use a current-thread Tokio runtime.
Complete the ordered gate, adversarial review, focused Jev probes for gap
probability ≥0.30, final-head checks, squash merge and merged-commit/issue readback.
The milestone stays open for every failed, skipped, unavailable, pending or
unsupported in-scope cell. All owner/prerequisite acceptance and #849 resource/
restart evidence must pass; close #791 last.

## Historical finite profile

The following material is retained from PR #877 as historical staged planning
and evidence. Its narrower selections, exclusions, approval status and delivery
policy describe that earlier snapshot. The selected contract above supersedes
them wherever they conflict; historical results are not fresh validation.

### Proposed finite SQL dialect profile

This proposed contract owns [#792](https://github.com/cntryl/cassie/issues/792) under [#791](https://github.com/cntryl/cassie/issues/791). Current planning refresh: repository revision `55b52e26` (2026-10-10); the detailed child issue source witnesses retain baseline `9ff42bc05c374f478a12a29caf7537268215e665`, and the prior source audit used `4a09c41a3670dad3a4c4c2b7ef7583327c37ab6b`. Those are historical mappings, not current execution. The ownership matrix inventories the 58 dialect child owners and their recorded prerequisites. Publication of this document does not approve a new public or durable contract, close an issue, or qualify runtime behavior. [Feature Support](feature-support.md) remains the sole current support owner; [Production Readiness](production-readiness.md) owns readiness.

#### Boundaries and approval

Cassie remains single-node, Midge remains the direct storage and recovery owner, pgwire remains primary, and REST remains secondary and administrative. Distributed SQL, replication, fleet coordination and arbitrary PostgreSQL extension loading remain outside this profile. Existing `cassie-midge-layout-v2` and accepted current type identities remain unchanged until an explicit successor decision.

[Query Engine Target 1](query-engine-target.md) remains the finite current-type execution target. Its exclusions are preserved for that target. The rows below propose later work for selected PostgreSQL-shaped syntax, exact types, routine/trigger execution, locking, transactional DDL and a selected vector identity profile. They do not retroactively remove Target 1 exclusions. Full PostgreSQL/server/catalog/extension parity and blanket OLTP certification remain unclaimed.

Approval is per finite family. The user selected the finite relation-alias AST/namespace and pagination expression contracts below on 2026-10-06 and explicitly authorized public API breaking changes. Existing-type syntax and scalar work may be approved independently of unresolved durable-type and transaction decisions. Native issue prerequisites must record this finite selection before dependent implementation; the full #792/#793 programs remain open. No wildcard “all PostgreSQL syntax” acceptance is permitted. Each row's implementation issue must pin the precise forms, type signatures, errors and exclusions before coding.

#### Finite capability matrix

This is the proposed compatibility envelope, not a claim that every row is implemented. For present behavior, use the linked [Feature Support](feature-support.md) and [PostgreSQL Compatibility](postgres-compatibility.md) contracts. A proposal or issue selection is not runtime support. Proposed expansion forms outside a selected row remain unclaimed or are rejected; accepted syntax must never silently discard an unsupported modifier or change its meaning.

| Area | Finite PostgreSQL-shaped scope | Owners | Decision and boundary |
| --- | --- | --- | --- |
| Lexing, expressions and names | Existing operators with whitespace-independent tokenization; ordinary relation aliases and column aliases; selected postfix casts/type names; null-safe predicates; ILIKE/regex; VALUES/row expressions; explicit CTE materialization; bounded LIMIT/OFFSET expressions. | #794–#801 | #794 is merged; #795 and #797 have separately selected finite contracts. #796/#798–#801 remain issue-scoped. Regex engine/Unicode behavior remains a gate for #799. No general PostgreSQL grammar claim. |
| Scalar functions and aggregates | Selected NULLIF/GREATEST/LEAST over existing scalar types; bounded string/formatting and numeric/calendar functions; aggregate modifiers, Boolean/statistical/ordered-set and collection aggregates. | #802–#806, #811, #813, #815 | #802 is selected only for its listed current types. Calendar, formatting, ordered-set, volatility, output-growth and coercion details stay bounded by each owner; exact numeric/temporal functions wait on #793/#812/#814. |
| Types, literals, casts and wire identity | Existing types remain exactly those in [Type Contract](type-contract.md). Proposed additions include exact NUMERIC, FLOAT4, TIMESTAMPTZ/INTERVAL, JSONB, standard arrays, enum/composite/domain, BIT and a selected vector profile. | #793, #807–#810, #812, #814, #816, #827–#828, #844 | #793 is a hard gate for each new family: names, OIDs, typmods, casts/parameters, text/binary codecs, NULL and equality laws, durable representation, limits and transition behavior must be explicitly resolved. No row here chooses those identities. |
| Relational and mutation execution | Array construction/indexing and UNNEST; JSON table functions; data-modifying CTEs; UPDATE FROM/DELETE USING; conflict targets; finite COPY forms. | #807–#809, #823, #825, #829, #834, #846 | Preserve statement visibility, written-row scope, atomicity, cancellation and bounded ownership. #834 waits on CTE/mutation/DDL foundations; #846 also waits on wire and type contracts. |
| Schema, catalog and extension lifecycle | Finite catalog rows; table creation/cloning; constraints/indexes; sequences/identity; temporary namespaces; atomic ALTER; LIST partitioning; object comments; allowlisted built-in extension metadata. | #817, #824, #826–#828, #831, #838–#845 | Preserve single-node operation and Midge ownership. Any persistent identity, layout, atomic publication, recovery, permission, or extension metadata change is gated by its owner contract; arbitrary extension loading is excluded. |
| Transactions, routines and locks | Selected snapshot/row/advisory-lock behavior, scoped settings, SQL routines, procedural subset, dynamic SQL, security context, triggers, transactional DDL. | #819–#822, #830–#837 | These features are not implied by PostgreSQL syntax acceptance. Snapshot visibility, lock lifecycle/deadlock/cancellation, routine trust/scope, trigger ordering, rollback and catalog publication need explicit finite contracts before implementation. |
| Catalog/client workflows | Allowlisted information-schema/pg_catalog rows and pinned generated ORM/client workflows over the selected SQL surface. | #817, #847 | Existing named-client support remains the narrow envelope in Feature Support and the compatibility probe contract. Client installation or connection alone is not workflow qualification; do not advertise unsupported catalog columns or PostgreSQL internals. |
| Conformance and operational evidence | Generic differential corpus for every selected profile cell; representative resource/restart workloads; release-ledger consolidation. | #848, #849, #781 | #848 follows contracts and implementations; #849 follows #848 plus operational prerequisites #8 and #781. Conformance does not promote Production readiness; #8/#29 and Production Readiness retain that authority. |

##### Delivery priority and gates

The work is ordered by contract dependencies, not by issue number. Finish or verify each phase's named prerequisite before starting dependent runtime work. Independent slices may be developed, published as PRs, reviewed and validated concurrently in isolated worktrees once their dependencies are satisfied. Keep each PR's required checks and review tied to its exact head. Serialize integration: squash-merge one ready PR at a time, then rebase dependent worktrees and rerun affected checks.

1. **Current behavior and finite syntax carveouts:** preserve Target 1, the current type contract, and existing parser/function behavior. The bounded #794 lexical prerequisite is merged. The selected #795 alias and #797 pagination contracts are narrow approved API changes; #798's selected predicate slice is separately bounded. Do not treat these selections as approval of the rest of #792.
2. **Existing-type SQL extensions:** work only from the child issue's pinned signatures, semantics and dependencies. Expressions and relational syntax over existing types may proceed when their source/AST/transaction prerequisites are complete; #796 remains dependent on #793 for new type spellings/modifiers.
3. **Extended type and durable identity:** before implementing any new family, resolve its #793 record. A versioned successor layout plus verified export/recreate/reimport is the selected common transition policy, but concrete per-family values, byte tags, OIDs, codecs, coercions, migration/recreate handling and limits remain unresolved. No general permission to choose these in an implementation follows from the transition policy.
4. **DDL, transaction, catalog and routine lifecycles:** proceed only after the affected snapshot, rollback, catalog publication, security and recovery contracts are selected. Names that appear in PostgreSQL syntax are insufficient evidence to choose atomicity or persistence behavior.
5. **Qualification and promotion:** run generic pinned-oracle #848 after all selected cells and named prerequisites have evidence. Run #849 only after its corpus and operational dependencies; Production-ready evidence remains under [Production Readiness](production-readiness.md), including #8, and local diagnostic promotion remains under #29.

| Priority | Scope | Exit condition |
| --- | --- | --- |
| P0 — correctness | Reproduced parser, binding, result, transaction or resource violations on currently claimed paths. | Focused regression and current support contract repaired before expanding dependent syntax. |
| P1 — selected capability | Explicitly selected finite additions and their required semantic/type/lifecycle contracts. | Owner issue's finite forms, descriptors, errors, NULL behavior, parameters, resource and lifecycle cases are specified and implemented or explicitly blocked. |
| P2 — qualification/readiness | Differential corpus, client workflows, representative workload/restart evidence and promotion records. | Exact-revision evidence satisfies each owner; unavailable or blocked axes stay visible and do not become support claims. |

The P0/P1/P2 labels are ordering categories, not deadline or completion estimates. A qualification row can close only with evidence; it need not produce a runtime change when the acceptance criteria are already met.

#### First wave: selected carveouts and proposed expansions

The #794/#802 rows record selected existing-contract carveouts. The finite #795/#797 public AST changes were selected by the user on 2026-10-06. None selects a stored type, durable layout, public batch interface or stronger transaction guarantee.

| Owner | Finite selection | Required boundaries |
| --- | --- | --- |
| #794 (selected finite slice) | Compact/spaced existing comparison operators =, !=, <>, <, >, <=, >= and arithmetic +, -, *, /; comments and quoted literals retain lexical boundaries. | Longest valid operator match; numeric exponent/sign boundaries; quoted operator text is inert; malformed tokens reject deterministically. No new operator/cast/type syntax, public AST, wire identity or stored format. Expanded casts remain #796-owned. |
| #795 after #794 | Explicit/implicit ordinary base-relation aliases, schema qualification, alias column lists and self-joins using existing join forms. | Alias hides original relation name; ambiguous references reject; quoted spelling remains exact; output labels and Describe/Execute agree; derived/CTE scopes remain intact. |
| #802 (selected finite slice) | NULLIF(a,b), GREATEST(a,b,...) and LEAST(a,b,...) over selected existing SMALLINT/INT/BIGINT/FLOAT/TEXT/BOOLEAN families. | PostgreSQL 18 coercion/result typing and NULL laws: NULLIF returns NULL on equality, otherwise first argument; extrema ignore NULL and return NULL if all inputs are NULL. NULLIF operator-selected first-operand return type, extrema common type and unknown/all-NULL descriptors require an explicit compatibility matrix; ARRAY/JSON/VECTOR are excluded; all-integer comparisons remain exact above 2^53; admitted mixed integer/FLOAT inputs follow PostgreSQL promoted FLOAT rounding. Temporal/CHAR/VARCHAR/ARRAY/JSON/VECTOR are excluded from this selected slice. Existing error priority and cancellation remain. |
| #797 | LIMIT/OFFSET integer literals, typed bound integer parameters and selected side-effect-free scalar integer expressions; LIMIT ALL and NULL. | NULL LIMIT means unbounded; NULL OFFSET means zero; negative and out-of-range reject; zero LIMIT returns no rows; checked integer conversion; stable ordering fixtures; scan/top-k/portal budgets use the same admitted bound. Correlated, aggregate, window and subquery bounds remain excluded until separately selected. |

Prepared parameter Describe must advertise the selected type without performing writes or publishing output. Admission, cancellation, deadline and result-cap failures preserve existing statement/transaction cleanup. NULLIF/extrema are not ordinary eager function aliases where PostgreSQL typing or evaluation semantics would differ; the chosen implementation must reproduce the oracle for admitted forms.

##### First-wave decision status

- #794: the bounded lexical carveout is selected in the issue; PR #852 was squash merged on 2026-10-07 to `3b03dd4db34ac88f533501d0484ecdc3cf223a2e`; #794 is closed and the merged source was read back. Scalar dollar-quoted literals, repeated unary-sign chains beyond a single signed numeric literal and chained/expanded postfix casts remain excluded. Protecting dollar-quoted text lexically does not admit it as a scalar literal. No broad #792 approval follows.
- #802: the selected finite existing-type coercion, unknown/all-NULL TEXT result, explicitly cast parameters and runtime eager-operand rules are recorded in #791/#802. They are not awaiting a new broad type approval. The now-merged native #794 prerequisite was required because compact function-argument operator suffixes can be truncated before evaluation; the 2026-10-06 issue comment records that witness. Inner COALESCE promotion before arithmetic is separately tracked by #850 and gates #761/#848.
- #795/#797: PR #858 completed the selected alias and pagination bundle and squash-merged on 2026-10-07 at `7977c848775ff4b776dbf40bda2bbb9893aac25d`. That commit is an ancestor of the refreshed `55b52e26` baseline; the merged source was read back and #795/#797 are closed. The historical candidate and validation notes below describe preparation evidence only. The broader #792/#793 profile, full generic dialect qualification and production-readiness obligations remain open.

Scalar oracle evidence: the preparation lane retained `target/dialect-scalars-evidence/oracle-results.txt`, `oracle.sql` and `postgres.sha256` in its isolated worktree. The local PostgreSQL 18.6 Homebrew executable is aarch64-apple-darwin25.6.0, Apple clang 21.0.0, SHA256 `db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`. Runtime column-divisor fixtures establish eager operand evaluation for NULLIF and extrema; a constant NULL first argument can be folded by PostgreSQL and is not a runtime laziness contract. Supplementary NULL-column fixtures preserve that distinction. Mixed BIGINT/FLOAT comparison follows the selected PostgreSQL promotion rather than claiming exact mixed-domain equality. This evidence does not approve #802 by itself.

#### Semantic oracle and evidence

Use PostgreSQL **18.6** as the proposed exact reference version, matching Target 1's PostgreSQL 18 comparison. Before first execution, retain server version and immutable container digest or build revision; the version alone is not a reproducible artifact. The scalar preparation lane executed reduced conditional cases on Homebrew PostgreSQL 18.6; that is limited semantic evidence, not completion of the full oracle corpus or immutable deployment qualification. A future patch-version change requires recorded rationale and corpus rerun. Cassie-specific identity/storage behavior uses the existing Cassie contract as authority, rather than assuming PostgreSQL parity.

| Family | Independent acceptance | Required recorded output |
| --- | --- | --- |
| Grammar/scope | Same generic fixtures on the pinned oracle, plus compact/spaced metamorphic forms and quoted/ambiguous names. | Acceptance or exact SQLSTATE, names, types, parameter positions; equal bags unless ORDER BY is declared. |
| Scalars/types | Hand-derived integer/NULL laws, pinned oracle, current codec goldens and malformed inputs. | Exact values, OIDs/typmods, text/binary packets, NULL identity, rounding/overflow/errors. |
| Relational/aggregates | Pinned oracle and authoritative scalar execution; duplicate/empty/all-NULL and ordering witnesses. | Multiplicity, declared order, types and cardinality errors; no FLOAT reassociation without a selected law. |
| Routines/security/catalog | Independent sessions, explicit role/search_path fixtures and allowlisted catalog rows. | Authorization, scope restoration, descriptors and no metadata leakage. |
| Transactions/DDL/locks | Deterministic interleaving barriers and independent-session pre/post state. | I/T/E protocol state, visibility, rollback/publication, lock release and restart state. |
| Resource/client/recovery | Hostile bounded inputs, cancellation/deadlines, pinned client traces and disk-backed restart. | Reservation release, no acknowledged data loss, exact revision/profile and passed/failed/unavailable classification. |

Every runtime issue needs focused should_ tests with Arrange/Act/Assert and red/green evidence, followed by the required build, locked full tests, full pedantic clippy, fmt and test-policy gates. Reused mappings and source inspection are not fresh execution. Finite corpus #848 must cover all selected child contracts and existing prerequisites before promotion; #849 additionally gates resource/restart claims.

#### Complete ownership matrix

All dialect expansion rows are planned or incompletely qualified at this baseline. Existing narrower support must be preserved; absent capability is not a regression. The current support matrix determines narrower status. Dependencies below are issue dependencies, not permission to bypass open prerequisites.

| Owner | Finite planned outcome | Prerequisites | Source authority |
| --- | --- | --- | --- |
| [#792](https://github.com/cntryl/cassie/issues/792) — Define the complete PostgreSQL-shaped dialect profile | Publish a finite grammar/type/function/DDL/transaction/catalog/client matrix, explicit architectural decisions, semantic oracles, priorities and delivery order. Reconcile existing limits for exact types, procedural routines, triggers, locking and transactional DDL before runtime work. Keep Midge direct and pgwire primary. Retain permanent single-node boundaries. | None | [docs/query-engine-target.md](query-engine-target.md) |
| [#793](https://github.com/cntryl/cassie/issues/793) — Specify extended SQL type, wire and storage contracts | Decide exact NUMERIC precision/scale, FLOAT4, TIMESTAMPTZ, INTERVAL, JSONB identity, standard arrays, enums/composites, BIT and vector extension identities. Specify OIDs, text/binary codecs, literal/parameter/cast rules, NULL meaning, durable representation, migration/recreate policy and resource limits before implementation. Existing aliases and custom OIDs are not new type support. | [#792](https://github.com/cntryl/cassie/issues/792) | [docs/type-contract.md](type-contract.md) |
| [#794](https://github.com/cntryl/cassie/issues/794) — Tokenize PostgreSQL operators independently of whitespace | Selected existing comparison/arithmetic operators parse independently of whitespace, preserving precedence, unary/exponent signs, quoted/dollar-quoted text, nested comments and deterministic invalid-token errors. No expanded cast/type or operator family; #796 owns expanded casts. | [#752](https://github.com/cntryl/cassie/issues/752), [#753](https://github.com/cntryl/cassie/issues/753), [#432](https://github.com/cntryl/cassie/issues/432), [#755](https://github.com/cntryl/cassie/issues/755) (closed; selected #794 slice only) | [src/sql/parser/expr.rs](../src/sql/parser/expr.rs) |
| [#795](https://github.com/cntryl/cassie/issues/795) — Bind ordinary relation aliases and qualified references | Support explicit and implicit base-table aliases, self-joins, schema-qualified relations, alias hiding, ambiguity errors and alias column lists without confusing derived/CTE namespaces. Preserve quoted spelling and output descriptors. | [#794](https://github.com/cntryl/cassie/issues/794); selected D1 contract | [src/sql/parser/query.rs](../src/sql/parser/query.rs) |
| [#796](https://github.com/cntryl/cassie/issues/796) — Complete PostgreSQL postfix cast and type-name syntax | Bind chained :: casts, parenthesized expressions, schema-qualified type names, modifiers and array suffixes equivalently to CAST. Reuse checked type conversion and reject unsupported type contracts instead of silently coercing. | [#794](https://github.com/cntryl/cassie/issues/794), [#793](https://github.com/cntryl/cassie/issues/793) | [src/sql/parser/expr.rs](../src/sql/parser/expr.rs) |
| [#797](https://github.com/cntryl/cassie/issues/797) — Bind parameters and expressions in LIMIT and OFFSET | Define and implement parameter/expression typing, NULL, zero, negative, overflow and all-row behavior. Apply bound values consistently to specialization, top-k, scans and cumulative portal limits; no allocation proportional to a hostile parameter. | Selected D2 contract over existing integer types | [src/sql/parser/query_select.rs](../src/sql/parser/query_select.rs) |
| [#798](https://github.com/cntryl/cassie/issues/798) — Implement null-safe comparison predicates | Implement IS DISTINCT FROM and IS NOT DISTINCT FROM with NULL-safe exact scalar/array/type semantics and planner legality. Cover literal and bound NULLs, indexed versus scanned execution and keyset predicates. | [#792](https://github.com/cntryl/cassie/issues/792) | [src/sql/parser/expr.rs](../src/sql/parser/expr.rs) |
| [#799](https://github.com/cntryl/cassie/issues/799) — Add ILIKE and PostgreSQL regular-expression predicates | Implement ILIKE, ~, ~*, !~, !~*, LIKE escape semantics and bounded regexp_replace behavior under an explicitly selected regex/Unicode contract. Handle NULL, malformed patterns, parameter binding, cancellation and resource limits; preserve existing LIKE. | [#792](https://github.com/cntryl/cassie/issues/792) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#800](https://github.com/cntryl/cassie/issues/800) — Support VALUES relations and row-value expressions | Add typed VALUES sources and CTE bodies, aliases/column lists, row constructors, tuple predicates and row/subquery arity validation. Cover mixed/NULL types, empty derived inputs, repeated tuples and exact BIGINT correspondence. | [#795](https://github.com/cntryl/cassie/issues/795), [#793](https://github.com/cntryl/cassie/issues/793) | [src/sql/parser/query.rs](../src/sql/parser/query.rs) |
| [#801](https://github.com/cntryl/cassie/issues/801) — Support explicit CTE materialization modifiers | Accept MATERIALIZED and NOT MATERIALIZED and specify observable evaluation, volatility, reference sharing, parameter typing and resource ownership; optimizer choices must preserve semantics. Retain recursive CTE behavior. | [#792](https://github.com/cntryl/cassie/issues/792) | [src/sql/parser/query.rs](../src/sql/parser/query.rs) |
| [#834](https://github.com/cntryl/cassie/issues/834) — Execute data-modifying CTEs with RETURNING relations | Specify one-statement snapshot, mutation order and visibility, RETURNING relation ownership and atomic failure across the complete CTE statement. Reject unsupported recursive mutation shapes explicitly. | [#801](https://github.com/cntryl/cassie/issues/801), [#823](https://github.com/cntryl/cassie/issues/823), [#833](https://github.com/cntryl/cassie/issues/833) | [src/sql/parser/query.rs](../src/sql/parser/query.rs) |
| [#802](https://github.com/cntryl/cassie/issues/802) — Add NULLIF, GREATEST and LEAST expressions | Implement common-type inference, exact comparisons, SQL NULL behavior, laziness where specified, parameters and descriptors across scalar/typed/aggregate/window expression boundaries. | #794 (closed; merged lexical prerequisite); #752/#753/#432/#755 (closed existing contracts) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#803](https://github.com/cntryl/cassie/issues/803) — Extend PostgreSQL string functions and formatting syntax | Add split_part, btrim/ltrim/rtrim, substr alias, replace, strpos/position, formatting and supported SUBSTRING/TRIM keyword forms. Define byte versus character indexing, format %I/%L/%s, NULL, invalid arguments and admitted output expansion. | [#792](https://github.com/cntryl/cassie/issues/792) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#813](https://github.com/cntryl/cassie/issues/813) — Add typed rounding and numeric scalar functions | Implement round/trunc, floor/ceil, mod and exponent/power forms required by the finite profile with separate exact-decimal and floating semantics, overflow/domain checks, signed zero, NULL and descriptors. | [#793](https://github.com/cntryl/cassie/issues/793), [#812](https://github.com/cntryl/cassie/issues/812) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#815](https://github.com/cntryl/cassie/issues/815) — Implement SQL clock and calendar functions | Implement NOW/CURRENT_DATE/CURRENT_TIMESTAMP, statement/transaction clocks, date_trunc/date_part/EXTRACT, to_char/to_date/to_timestamp and age under the temporal contract. Test calendar boundaries, microseconds, time zones, DST, invalid input and volatility/cache behavior. | [#793](https://github.com/cntryl/cassie/issues/793), [#814](https://github.com/cntryl/cassie/issues/814) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#804](https://github.com/cntryl/cassie/issues/804) — Support aggregate DISTINCT, FILTER and ordered arguments | Represent aggregate-local DISTINCT, FILTER and aggregate-local ORDER BY explicitly. Apply filtering before transitions and preserve DISTINCT, NULL, grouping/HAVING, empty-group behavior and exact order. Scalar, typed, parallel and encoded paths must agree or select a documented fallback. | [#792](https://github.com/cntryl/cassie/issues/792), [#758](https://github.com/cntryl/cassie/issues/758) | [src/sql/ast.rs](../src/sql/ast.rs) |
| [#805](https://github.com/cntryl/cassie/issues/805) — Add BOOL_AND and BOOL_OR aggregates | Implement SQL NULL and empty/all-NULL behavior, Boolean-only typing and correct partial-state merge. Cover FILTER, groups, exact source paths, cancellation and terminal errors without partial output. | [#804](https://github.com/cntryl/cassie/issues/804) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#811](https://github.com/cntryl/cassie/issues/811) — Add ordered string, array and JSON collection aggregates | Implement STRING_AGG, ARRAY_AGG and JSONB_AGG/JSONB_OBJECT_AGG with delimiters, duplicates, ordered/DISTINCT arguments, NULL/empty rules, typed results and incremental admission before retained growth. Preserve aggregate result indexing and scalar/typed correspondence. | [#804](https://github.com/cntryl/cassie/issues/804), [#807](https://github.com/cntryl/cassie/issues/807), [#810](https://github.com/cntryl/cassie/issues/810), [#793](https://github.com/cntryl/cassie/issues/793) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#806](https://github.com/cntryl/cassie/issues/806) — Add statistical and ordered-set aggregates | Implement finite percentile_cont/percentile_disc, standard deviation and variance forms, including ordered-set syntax, NULL filtering, interpolation, sample/population denominators, deterministic exactness policy, resource limits and partial-state merges. | [#804](https://github.com/cntryl/cassie/issues/804), [#793](https://github.com/cntryl/cassie/issues/793) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#807](https://github.com/cntryl/cassie/issues/807) — Add PostgreSQL array constructors, predicates and indexing | Implement ARRAY literals/subqueries, subscripts/slices, cardinality/array_length, ANY/ALL, containment/overlap and array concatenation under the selected dimensional/lower-bound contract. Preserve NULL array versus NULL element versus empty array, standard bindings and exact descriptors. | [#793](https://github.com/cntryl/cassie/issues/793), [#796](https://github.com/cntryl/cassie/issues/796), [#762](https://github.com/cntryl/cassie/issues/762) | [src/sql/parser/expr.rs](../src/sql/parser/expr.rs) |
| [#808](https://github.com/cntryl/cassie/issues/808) — Execute UNNEST and table-function ordinality | Add admitted UNNEST sources, alias column lists, WITH ORDINALITY, lateral correlation, multiple-array length rules and LEFT JOIN empty-source behavior. Advance under cancellation/LIMIT/resource bounds without eager expansion. | [#807](https://github.com/cntryl/cassie/issues/807), [#795](https://github.com/cntryl/cassie/issues/795) | [src/executor/execution/source.rs](../src/executor/execution/source.rs) |
| [#809](https://github.com/cntryl/cassie/issues/809) — Implement PostgreSQL JSON operators and path predicates | Implement ->, ->>, #>, #>>, key existence ?/?\|/?&, containment, concatenation and deletion under the JSON/JSONB contract. Distinguish missing paths, SQL NULL, document null, unsigned/exact numbers, object equality/order and malformed casts. | [#793](https://github.com/cntryl/cassie/issues/793), [#794](https://github.com/cntryl/cassie/issues/794) | [src/sql/parser/expr.rs](../src/sql/parser/expr.rs) |
| [#810](https://github.com/cntryl/cassie/issues/810) — Add JSON inspection, construction and conversion functions | Implement jsonb_typeof/array_length, build_object/build_array, to_jsonb, jsonb_exists_any/all, jsonb_object_keys and finite path/set helpers with correct arity/key validation, NULL distinctions and bounded expansion. Record same-type JSON aliases separately from a true JSONB ABI. | [#809](https://github.com/cntryl/cassie/issues/809), [#793](https://github.com/cntryl/cassie/issues/793) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#829](https://github.com/cntryl/cassie/issues/829) — Execute JSON element and recordset table functions | Implement jsonb_array_elements/_text, jsonb_each/_text, jsonb_to_recordset and record conversion with declared output columns. Preserve lateral/ordinality behavior, missing fields, JSON null, conversion errors, source lifetime and expansion admission; no partial result after later malformed input. | [#810](https://github.com/cntryl/cassie/issues/810), [#808](https://github.com/cntryl/cassie/issues/808), [#828](https://github.com/cntryl/cassie/issues/828) | [src/executor/execution/source.rs](../src/executor/execution/source.rs) |
| [#812](https://github.com/cntryl/cassie/issues/812) — Implement exact NUMERIC and DECIMAL values | Implement the approved exact-decimal carrier, precision/scale modifiers, rounding/overflow, arithmetic/comparison/aggregate typing, literal/bind/cast/RETURNING rules, text/binary OID 1700 codecs, indexes and restart behavior. Never map a declared exact decimal to FLOAT8. | [#793](https://github.com/cntryl/cassie/issues/793) | [src/types/schema.rs](../src/types/schema.rs) |
| [#814](https://github.com/cntryl/cassie/issues/814) — Implement TIMESTAMPTZ and calendar INTERVAL semantics | Implement TIMESTAMPTZ, INTERVAL, AT TIME ZONE, offset/named-zone parsing, calendar month/day/microsecond arithmetic and comparison. Pin precision, DST overlap/gap, month-end, text/binary OIDs, stored normalization and session TimeZone behavior. Preserve the existing fixed-duration time_bucket contract. | [#793](https://github.com/cntryl/cassie/issues/793) | [src/types/schema.rs](../src/types/schema.rs) |
| [#827](https://github.com/cntryl/cassie/issues/827) — Add named enum types and typed enum values | Implement named ENUM creation/lookup/alter/drop and enum literal/parameter/cast/comparison/array/result identities. Define schema scoping, label ordering, dependency ownership, restart and migration compatibility before publishing durable values. | [#793](https://github.com/cntryl/cassie/issues/793), [#826](https://github.com/cntryl/cassie/issues/826) | [src/sql/ast.rs](../src/sql/ast.rs) |
| [#828](https://github.com/cntryl/cassie/issues/828) — Add composite, record and domain type semantics | Implement finite composite/record field access, relation row types, typed recordset output and domains with constraint enforcement. Define composite/domain OIDs, row NULL versus all-NULL fields, schema dependencies, ownership and persistent compatibility. | [#793](https://github.com/cntryl/cassie/issues/793), [#826](https://github.com/cntryl/cassie/issues/826) | [src/sql/ast.rs](../src/sql/ast.rs) |
| [#816](https://github.com/cntryl/cassie/issues/816) — Add bit-string, bytea and hashing conversion functions | Implement BIT/VARBIT length and signedness, bytea operators and finite md5/sha256/digest, encode/decode, convert_to/convert_from, hashtext/hashtextextended behavior. Pin PostgreSQL-compatible outputs, encodings, invalid-input errors and bounds; reject unsupported algorithms explicitly. | [#793](https://github.com/cntryl/cassie/issues/793), [#796](https://github.com/cntryl/cassie/issues/796), [#803](https://github.com/cntryl/cassie/issues/803) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#844](https://github.com/cntryl/cassie/issues/844) — Support the selected pgvector SQL and type identity profile | Implement bare vector casts with dimension inference, pgvector text/binary/type discovery and metric/operator-class identities under the approved profile. Ensure extension detection agrees with actual support and preserves negative inner product/order, NULL/error rules and ANN/exact correspondence. | [#793](https://github.com/cntryl/cassie/issues/793), [#797](https://github.com/cntryl/cassie/issues/797), [#843](https://github.com/cntryl/cassie/issues/843), [#776](https://github.com/cntryl/cassie/issues/776) | [docs/type-contract.md](type-contract.md) |
| [#817](https://github.com/cntryl/cassie/issues/817) — Extend finite PostgreSQL relation, type and routine catalogs | Inventory and implement finite pg_type/pg_enum/pg_namespace/pg_class/pg_attribute/pg_proc/pg_constraint/pg_depend/pg_sequence/partition catalog columns required by the dialect. Rows must reflect real objects, OIDs, owners, defaults, constraints and active database scope; do not manufacture unsupported capabilities. | [#793](https://github.com/cntryl/cassie/issues/793), [#792](https://github.com/cntryl/cassie/issues/792), [#777](https://github.com/cntryl/cassie/issues/777) | [src/catalog/virtual_views.rs](../src/catalog/virtual_views.rs) |
| [#818](https://github.com/cntryl/cassie/issues/818) — Resolve PostgreSQL object identifier pseudo-types | Add finite regclass/regnamespace/regtype/regprocedure conversions and to_reg* lookup helpers with search_path, quoted names, overload signatures, missing-object behavior, OID identity, database scope and privilege-aware visibility. | [#817](https://github.com/cntryl/cassie/issues/817), [#796](https://github.com/cntryl/cassie/issues/796) | [src/types/schema.rs](../src/types/schema.rs) |
| [#831](https://github.com/cntryl/cassie/issues/831) — Complete object ownership and privilege DDL | Implement ALTER OWNER, routine/sequence/schema/table privileges, ALTER DEFAULT PRIVILEGES, role membership/attributes and corresponding has_* / pg_has_role checks for the selected finite profile. Enforce live authorization and ownership transitions rather than parsing privilege statements as no-ops. | [#792](https://github.com/cntryl/cassie/issues/792), [#830](https://github.com/cntryl/cassie/issues/830), [#817](https://github.com/cntryl/cassie/issues/817) | [src/sql/parser/statements.rs](../src/sql/parser/statements.rs) |
| [#830](https://github.com/cntryl/cassie/issues/830) — Extend SQL routines with signatures and set-returning results | Add schema-qualified overload signatures, declared/default/named arguments, RETURNS TABLE/SETOF and scalar/composite output, CREATE OR REPLACE and signature-aware ALTER/DROP. Enforce invocation types, volatility, recursion/depth, cancellation and row/memory bounds. Retain existing scalar UDF and narrow CALL behavior. | [#792](https://github.com/cntryl/cassie/issues/792), [#828](https://github.com/cntryl/cassie/issues/828), [#795](https://github.com/cntryl/cassie/issues/795) | [src/sql/parser/statements.rs](../src/sql/parser/statements.rs) |
| [#835](https://github.com/cntryl/cassie/issues/835) — Execute the selected procedural SQL language subset | Implement the selected DECLARE/BEGIN/END, typed variables/records, assignment, IF/CASE, FOR/WHILE/LOOP, SELECT INTO, PERFORM, RETURN/RETURN NEXT/RETURN QUERY, FOUND/GET DIAGNOSTICS and RAISE/EXCEPTION semantics. Pin block/subtransaction behavior and execution budgets; unsupported constructs must fail before side effects. | [#830](https://github.com/cntryl/cassie/issues/830), [#833](https://github.com/cntryl/cassie/issues/833) | [src/sql/parser/statements.rs](../src/sql/parser/statements.rs) |
| [#836](https://github.com/cntryl/cassie/issues/836) — Add anonymous procedural blocks and parameterized dynamic SQL | Execute selected DO blocks and EXECUTE ... USING with correct identifier/literal formatting, search_path, plan binding, result/INTO shape, privileges and transactional rollback. Parse dollar quotes/multiple statements correctly and prevent unsupported embedded syntax from partially executing. | [#835](https://github.com/cntryl/cassie/issues/835), [#803](https://github.com/cntryl/cassie/issues/803) | [src/sql/parser/statements.rs](../src/sql/parser/statements.rs) |
| [#832](https://github.com/cntryl/cassie/issues/832) — Enforce routine security context and scoped settings | Implement invoker/definer identities, EXECUTE grants, routine SET settings, owner changes and restored caller state through nested calls/errors. Pin resolution and privilege checks for dynamic SQL and trigger invocation without a privilege or search_path bypass. | [#830](https://github.com/cntryl/cassie/issues/830), [#831](https://github.com/cntryl/cassie/issues/831), [#821](https://github.com/cntryl/cassie/issues/821) | [src/catalog/mod.rs](../src/catalog/mod.rs) |
| [#837](https://github.com/cntryl/cassie/issues/837) — Add transactional row-trigger and constraint-trigger execution | Select and implement BEFORE/AFTER row triggers, OLD/NEW/TG_* context, returned-row/suppression behavior, WHEN predicates, trigger enable/drop and finite deferred constraint-trigger semantics. Apply uniformly to SQL/REST/COPY/upsert with statement atomicity, rollback, restart ownership, bounded recursion and exactly-once effects. This requires the profile to resolve the current trigger boundary first. | [#792](https://github.com/cntryl/cassie/issues/792), [#835](https://github.com/cntryl/cassie/issues/835), [#832](https://github.com/cntryl/cassie/issues/832), [#833](https://github.com/cntryl/cassie/issues/833) | [src/sql/ast.rs](../src/sql/ast.rs) |
| [#822](https://github.com/cntryl/cassie/issues/822) — Add session and transaction advisory locks | Implement finite 64-bit/two-key, shared/exclusive, try/blocking session/xact advisory lock forms with ownership, reentrancy, commit/rollback/disconnect release, cancellation, deadlines and fair bounded waits. Test two sessions and prevent a successful no-op lock implementation. | [#792](https://github.com/cntryl/cassie/issues/792), [#821](https://github.com/cntryl/cassie/issues/821), [#816](https://github.com/cntryl/cassie/issues/816), [#763](https://github.com/cntryl/cassie/issues/763) | [src/sql/functions.rs](../src/sql/functions.rs) |
| [#820](https://github.com/cntryl/cassie/issues/820) — Support row locking and SKIP LOCKED claims | Define and implement finite FOR UPDATE/NO KEY UPDATE/SHARE/KEY SHARE, OF, NOWAIT and SKIP LOCKED clauses, selection ordering, lock rechecks, staged visibility, update/delete conflicts and cleanup. Two-session claims must neither duplicate work nor silently ignore locks. | [#792](https://github.com/cntryl/cassie/issues/792), [#819](https://github.com/cntryl/cassie/issues/819), [#763](https://github.com/cntryl/cassie/issues/763) | [src/sql/ast.rs](../src/sql/ast.rs) |
| [#819](https://github.com/cntryl/cassie/issues/819) — Add the selected snapshot isolation query profile | Decide and implement REPEATABLE READ/serializable requirements and read-only transaction modes under a direct-Midge contract, including multi-statement snapshots, write conflict rules, failures and portal handoff. Current READ COMMITTED-only behavior remains authoritative until this profile is specified; no unsupported isolation keyword may be accepted as an alias. | [#792](https://github.com/cntryl/cassie/issues/792), [#763](https://github.com/cntryl/cassie/issues/763) | [docs/pgwire-transaction-contract.md](pgwire-transaction-contract.md) |
| [#821](https://github.com/cntryl/cassie/issues/821) — Extend scoped PostgreSQL transaction and session settings | Implement selected SET/RESET/SET LOCAL and startup options for search_path, TimeZone, default_transaction_read_only, transaction_read_only, statement_timeout and lock_timeout. Preserve restoration at rollback/savepoints/routine return and enforce modes/deadlines in every path. Distinguish accepted compatibility-only settings from enforced semantics. | [#792](https://github.com/cntryl/cassie/issues/792), [#814](https://github.com/cntryl/cassie/issues/814), [#780](https://github.com/cntryl/cassie/issues/780) | [src/app/session.rs](../src/app/session.rs) |
| [#823](https://github.com/cntryl/cassie/issues/823) — Execute UPDATE FROM and DELETE USING statements | Add joined mutation sources, aliases, tuple assignments and DELETE USING with scope/type/cardinality rules, conflict behavior and exact RETURNING descriptors. Admit candidate state before mutation; preserve staging, constraints, cancellation and atomic failures across all physical paths. | [#795](https://github.com/cntryl/cassie/issues/795), [#800](https://github.com/cntryl/cassie/issues/800), [#792](https://github.com/cntryl/cassie/issues/792), [#769](https://github.com/cntryl/cassie/issues/769) | [src/sql/parser/dml.rs](../src/sql/parser/dml.rs) |
| [#825](https://github.com/cntryl/cassie/issues/825) — Complete PostgreSQL conflict target and conditional upsert syntax | Extend existing ON CONFLICT with named constraints, expression/partial-index inference and conditional DO UPDATE. Preserve omitted/default values, excluded typing, statement order, no-op RETURNING counts, unique-key races and transaction visibility. | [#823](https://github.com/cntryl/cassie/issues/823), [#798](https://github.com/cntryl/cassie/issues/798), [#824](https://github.com/cntryl/cassie/issues/824) | [src/sql/parser/dml.rs](../src/sql/parser/dml.rs) |
| [#826](https://github.com/cntryl/cassie/issues/826) — Add PostgreSQL table creation and cloning forms | Implement finite LIKE INCLUDING/EXCLUDING, CREATE TABLE AS SELECT, typed defaults, generated/identity declarations and named constraints. Define copied metadata/dependencies and single-statement atomic catalog+Midge publication; do not silently ignore modifiers or storage choices. | [#793](https://github.com/cntryl/cassie/issues/793), [#792](https://github.com/cntryl/cassie/issues/792), [#824](https://github.com/cntryl/cassie/issues/824) | [src/sql/parser/schema.rs](../src/sql/parser/schema.rs) |
| [#838](https://github.com/cntryl/cassie/issues/838) — Add session-local temporary tables and namespaces | Implement TEMP/TEMPORARY, pg_temp resolution, session isolation, ON COMMIT behavior, temporary indexes and routine access. Release objects on disconnect/cancellation and respect permissions, quotas and statement/transaction boundaries. | [#826](https://github.com/cntryl/cassie/issues/826), [#833](https://github.com/cntryl/cassie/issues/833) | [src/sql/parser/schema.rs](../src/sql/parser/schema.rs) |
| [#824](https://github.com/cntryl/cassie/issues/824) — Complete named constraint lifecycle and validation syntax | Extend supported constraints with named ADD/DROP/VALIDATE, explicit NOT DEFERRABLE, selected deferrability and existing-row validation. Specify FK MATCH/actions, unique NULL policy and ALTER sequencing. Verify catalog dependencies, all ingress paths, existing data failures and atomic rollback. | [#792](https://github.com/cntryl/cassie/issues/792), [#769](https://github.com/cntryl/cassie/issues/769) | [src/sql/parser/schema_table_constraints.rs](../src/sql/parser/schema_table_constraints.rs) |
| [#839](https://github.com/cntryl/cassie/issues/839) — Support atomic ALTER TYPE and multi-action table changes | Support selected column type/USING transformations, ADD COLUMN IF NOT EXISTS, multiple actions, schema moves and CASCADE/RESTRICT dependency handling. Define row conversion/rewrite and rollback/restart behavior before any persistent layout change; preserve live identifier/type metadata. | [#793](https://github.com/cntryl/cassie/issues/793), [#824](https://github.com/cntryl/cassie/issues/824), [#833](https://github.com/cntryl/cassie/issues/833) | [src/sql/parser/schema_sequences.rs](../src/sql/parser/schema_sequences.rs) |
| [#833](https://github.com/cntryl/cassie/issues/833) — Stage selected DDL changes transactionally | Specify selected transactional CREATE/ALTER/DROP, visibility, savepoints and catalog/data atomic handoff through Midge. Extend the current standalone-DDL wire boundary only through the approved profile; fail or roll back complete operations rather than leaking committed catalog changes. | [#792](https://github.com/cntryl/cassie/issues/792), [#763](https://github.com/cntryl/cassie/issues/763) | [docs/pgwire-transaction-contract.md](pgwire-transaction-contract.md) |
| [#840](https://github.com/cntryl/cassie/issues/840) — Add LIST-partitioned relation lifecycle and routing | Implement selected PARTITION BY LIST/PARTITION OF, default partition, ATTACH/DETACH, inherited schema/constraints/indexes, routing, pruning, parent scans and atomic lifecycle/recovery. Specify storage layout and migration decisions first; keep physical partition ownership in direct Midge integration. | [#793](https://github.com/cntryl/cassie/issues/793), [#826](https://github.com/cntryl/cassie/issues/826), [#833](https://github.com/cntryl/cassie/issues/833), [#771](https://github.com/cntryl/cassie/issues/771) | [src/sql/ast.rs](../src/sql/ast.rs) |
| [#841](https://github.com/cntryl/cassie/issues/841) — Extend sequence options, identity and value functions | Extend existing sequences/nextval with finite CREATE/ALTER options, OWNED BY, identity dependency, setval/currval/lastval and regclass lookup. Pin nontransactional allocation versus transactional catalog behavior, permissions, concurrency, restart and DROP/TRUNCATE effects. | [#793](https://github.com/cntryl/cassie/issues/793), [#818](https://github.com/cntryl/cassie/issues/818), [#826](https://github.com/cntryl/cassie/issues/826) | [src/sql/parser/schema_sequences.rs](../src/sql/parser/schema_sequences.rs) |
| [#842](https://github.com/cntryl/cassie/issues/842) — Complete PostgreSQL index DDL and operator-class contracts | Extend existing btree/hash/full-text/vector indexes with standard DROP without ON, schema scoping, ordered key modifiers, INCLUDE and selected operator-class/options syntax. Define which methods/classes are implemented equivalents and reject unsupported semantics; validate index rebuild/partial predicate/NULL coverage without changing query answers. | [#792](https://github.com/cntryl/cassie/issues/792), [#793](https://github.com/cntryl/cassie/issues/793), [#768](https://github.com/cntryl/cassie/issues/768) | [src/sql/parser/schema_indexes.rs](../src/sql/parser/schema_indexes.rs) |
| [#843](https://github.com/cntryl/cassie/issues/843) — Add an explicit built-in extension compatibility catalog | Define finite built-in extension identities (for selected vector and crypto functions), CREATE EXTENSION IF NOT EXISTS behavior, schemas/version metadata and pg_extension/pg_available_extensions rows. Advertise only implemented capabilities; no claim of loading arbitrary PostgreSQL extension libraries. | [#792](https://github.com/cntryl/cassie/issues/792), [#817](https://github.com/cntryl/cassie/issues/817) | [src/catalog/virtual_views.rs](../src/catalog/virtual_views.rs) |
| [#845](https://github.com/cntryl/cassie/issues/845) — Persist SQL object comments and identity metadata | Implement selected COMMENT ON/IS NULL for schemas/tables/columns/routines/types and matching obj_description/col_description/catalog reads. Preserve ownership, database scope, restore/restart behavior and dependency cleanup. | [#792](https://github.com/cntryl/cassie/issues/792), [#817](https://github.com/cntryl/cassie/issues/817) | [src/sql/parser/statements.rs](../src/sql/parser/statements.rs) |
| [#846](https://github.com/cntryl/cassie/issues/846) — Extend finite PostgreSQL COPY and migration I/O forms | Extend existing CSV ingress with the selected COPY query/table TO forms, options, exact type encodings and finite binary/array/NULL handling. Qualify bounded streaming, cancellation/drain, staged statement atomicity and malformed input without claiming PostgreSQL pg_dump/pg_restore parity. | [#793](https://github.com/cntryl/cassie/issues/793), [#792](https://github.com/cntryl/cassie/issues/792), [#780](https://github.com/cntryl/cassie/issues/780) | [src/sql/parser/copy.rs](../src/sql/parser/copy.rs) |
| [#847](https://github.com/cntryl/cassie/issues/847) — Qualify generated PostgreSQL ORM queries against the dialect profile | Add generic pinned Prisma 7 PostgreSQL-adapter raw and delegate fixtures: CRUD/upsert, pagination, arrays/JSON/decimal/time values, enums, aggregate/groupBy, transactions, schema discovery and errors. Capture generated SQL and actual parameter/result OIDs; verify semantics rather than SELECT 1 or install success. Keep existing named-client profiles distinct. | [#795](https://github.com/cntryl/cassie/issues/795), [#797](https://github.com/cntryl/cassie/issues/797), [#796](https://github.com/cntryl/cassie/issues/796), [#793](https://github.com/cntryl/cassie/issues/793), [#817](https://github.com/cntryl/cassie/issues/817), [#777](https://github.com/cntryl/cassie/issues/777) | [docs/compatibility-probe-contract.md](compatibility-probe-contract.md) |
| [#848](https://github.com/cntryl/cassie/issues/848) — Qualify the complete generic SQL dialect corpus | Retain generic seeded parameterized syntax/semantic fixtures for every profile cell and compare with a pinned PostgreSQL oracle. Exercise scalar/typed/indexed/encoded/overlay/REST/wire boundaries, NULL/error/type identity, scalar cardinality, volatility, later errors and cleanup. Every cell needs executed evidence, a documented exclusion or an unresolved blocker; no application-specific fixtures. | [#792](https://github.com/cntryl/cassie/issues/792), [#793](https://github.com/cntryl/cassie/issues/793), [#794](https://github.com/cntryl/cassie/issues/794), [#795](https://github.com/cntryl/cassie/issues/795), [#796](https://github.com/cntryl/cassie/issues/796), [#797](https://github.com/cntryl/cassie/issues/797), [#798](https://github.com/cntryl/cassie/issues/798), [#799](https://github.com/cntryl/cassie/issues/799), [#800](https://github.com/cntryl/cassie/issues/800), [#801](https://github.com/cntryl/cassie/issues/801), [#834](https://github.com/cntryl/cassie/issues/834), [#802](https://github.com/cntryl/cassie/issues/802), [#803](https://github.com/cntryl/cassie/issues/803), [#813](https://github.com/cntryl/cassie/issues/813), [#815](https://github.com/cntryl/cassie/issues/815), [#804](https://github.com/cntryl/cassie/issues/804), [#805](https://github.com/cntryl/cassie/issues/805), [#811](https://github.com/cntryl/cassie/issues/811), [#806](https://github.com/cntryl/cassie/issues/806), [#807](https://github.com/cntryl/cassie/issues/807), [#808](https://github.com/cntryl/cassie/issues/808), [#809](https://github.com/cntryl/cassie/issues/809), [#810](https://github.com/cntryl/cassie/issues/810), [#829](https://github.com/cntryl/cassie/issues/829), [#812](https://github.com/cntryl/cassie/issues/812), [#814](https://github.com/cntryl/cassie/issues/814), [#827](https://github.com/cntryl/cassie/issues/827), [#828](https://github.com/cntryl/cassie/issues/828), [#816](https://github.com/cntryl/cassie/issues/816), [#844](https://github.com/cntryl/cassie/issues/844), [#817](https://github.com/cntryl/cassie/issues/817), [#818](https://github.com/cntryl/cassie/issues/818), [#831](https://github.com/cntryl/cassie/issues/831), [#830](https://github.com/cntryl/cassie/issues/830), [#835](https://github.com/cntryl/cassie/issues/835), [#836](https://github.com/cntryl/cassie/issues/836), [#832](https://github.com/cntryl/cassie/issues/832), [#837](https://github.com/cntryl/cassie/issues/837), [#822](https://github.com/cntryl/cassie/issues/822), [#820](https://github.com/cntryl/cassie/issues/820), [#819](https://github.com/cntryl/cassie/issues/819), [#821](https://github.com/cntryl/cassie/issues/821), [#823](https://github.com/cntryl/cassie/issues/823), [#825](https://github.com/cntryl/cassie/issues/825), [#826](https://github.com/cntryl/cassie/issues/826), [#838](https://github.com/cntryl/cassie/issues/838), [#824](https://github.com/cntryl/cassie/issues/824), [#839](https://github.com/cntryl/cassie/issues/839), [#833](https://github.com/cntryl/cassie/issues/833), [#840](https://github.com/cntryl/cassie/issues/840), [#841](https://github.com/cntryl/cassie/issues/841), [#842](https://github.com/cntryl/cassie/issues/842), [#843](https://github.com/cntryl/cassie/issues/843), [#845](https://github.com/cntryl/cassie/issues/845), [#846](https://github.com/cntryl/cassie/issues/846), [#847](https://github.com/cntryl/cassie/issues/847), [#762](https://github.com/cntryl/cassie/issues/762), [#761](https://github.com/cntryl/cassie/issues/761), [#779](https://github.com/cntryl/cassie/issues/779), [#850](https://github.com/cntryl/cassie/issues/850) | [docs/query-promotion-evidence.md](query-promotion-evidence.md) |
| [#849](https://github.com/cntryl/cassie/issues/849) — Establish dialect workload resource and restart profiles | Define generic representative relational/reporting/JSON-expansion/mutation/coordination/client workloads and measure latency, capacity, memory, cancellation, contention and restart/recovery at named scales on one immutable commit. Validate acknowledged state and publication invariants. Separate semantic completion from production promotion. | [#848](https://github.com/cntryl/cassie/issues/848), [#8](https://github.com/cntryl/cassie/issues/8), [#781](https://github.com/cntryl/cassie/issues/781) | [docs/performance-contracts.md](performance-contracts.md) |

#### Proposed prerequisite refinement

The integration owner explicitly amended #794 and removed its broad #792 blocker before implementation, selecting the finite existing-operator lexical slice: it preserves selected #752/#753/#432/#755 types, NULL/error/resource/wire contracts, and adds no operator family, public AST or storage identity. Expanded operator families remain profile-gated.

The integration owner selected #802 finite SMALLINT/INT/BIGINT/FLOAT/TEXT/BOOLEAN signatures, PostgreSQL 18.6 coercion/NULL/eager-evaluation cases and explicitly cast parameter inputs, then removed its broad #792 blocker before implementation. New type families or unresolved coercion forms remain #792/#793-gated. The selected #802 slice reuses closed #752/#753/#432/#755 contracts. The #794 selection does not close or approve the full #792 profile. #794 is now merged and closed after the compact-argument lexical finding. #795 has the finite public alias AST/namespace contract selected below, and its #794 runtime prerequisite is now merged and closed.

#### Historical first-wave delivery and ownership

The following two paragraphs preserve the historical first-wave preparation record, including issue states and the former publication policy. The current workflow is defined in [Delivery priority and gates](#delivery-priority-and-gates).

The integration owner owns contract review, shared AST/catalog/storage decisions, the dependency graph, exact-head review and serial publication/merge. Persistent sibling worktrees have separate target directories and server ports. Current #851 merged scan/filter/projection and portal qualification before this refresh; #756/#757 are closed and must not be rebuilt as missing foundation. Later #758/#759/#760 typed aggregates, joins and ordering/set/window kernels remain open. First-wave lanes are syntax/binding (#794 then #795), scalar expressions (#802) and pagination (#797). Parser changes are syntax-owned; function metadata/evaluator changes are scalar-owned; pagination isolates its helper and integration reviews shared query AST changes. Null-safe predicate grammar #798 follows the syntax merge.

After the first wave, aggregate modifiers/Boolean/collection/statistical operators, JSON expressions and approved extended types can proceed in separate lanes only after their native prerequisites close. Array/table expansion follows its type/source prerequisites. Routines, triggers, isolation, transactional DDL and partitions remain blocked on selected lifecycle and persistence decisions. Each worker delivers one coherent bundle; integration publishes one PR at a time, verifies its final head and squash-merges, then remaining workers rebase and rerun affected checks.

#### Contract selections and remaining gates

[#793](https://github.com/cntryl/cassie/issues/793) owns the
[type and durable-format decisions](extended-type-decisions.md). Full PostgreSQL
semantics, the common CRB3 direction and the separately approved catalog/export
schemas are selected. The family/resolution records define that target.
Complete each affected record and recorded prerequisite before dependent runtime.

| Decision | Required resolution | Blocked owners |
| --- | --- | --- |
| Extended types/ABI | Selected full PostgreSQL families, common v3 identities/codecs and recreate transition; selected exact catalog/export schemas; staged extension-specific records remain required. | #793 and its typed dependents. |
| Temporal contract | Full PostgreSQL calendar/month, named-zone/DST and clock semantics; pinned 2026c source data, provider artifacts and actual qualification. | #814/#815. |
| Regex/text collation | Engine/Unicode version, collation/case rules, pattern budget and bounded execution. | #799/#803. |
| Procedural language | Full PL/pgSQL, dynamic SQL binding, recursion/resource bounds and trusted-language security; earlier subsets are intermediate. | #830/#832/#835/#836/#837. |
| Locks/isolation | Snapshot level, row identity/conflicts, deadlock policy, lock lifecycle and cancellation. | #819/#820/#822. |
| Transactional catalog changes | Atomic schema publication, durable recovery, catalog/plan invalidation and concurrent readers. | #833/#837/#838/#839/#840. |
| Extension identity | Allowlisted built-in identities and explicit compatibility subset; no arbitrary plugin ABI. | #843/#844. |
| Complete corpus | Every in-scope form/signature/context and pinned client workflow; unresolved forms remain pending and keep the milestone open. | #847/#848/#849. |

Issue #792 remains open for the broad profile. The selected finite decisions below do not complete #793 or qualify any pending runtime child.

The 2026-10-08 selections additionally authorize the finite query/DML null-safe
predicate slice in #798 with persisted-definition rejection, and one shared
same-database Data statement snapshot in #763 while preserving suspended portals.
The user also permits breaking durable changes: #793 now selects an explicitly
versioned successor layout with verified export/recreate/reimport, as recorded in
[the transition policy](extended-type-decisions.md). These selections resolve
their named decision gates. They do not implement or qualify those capabilities,
complete all family/catalog/export records, or close the broad #792/#793 programs.

The 2026-10-10 approval subsequently selects the concrete
[common CRB3/type identity/codec direction](postgresql18-v3-contract.md).
The user subsequently selected the concrete
[catalog/export schemas](postgresql18-catalog-export-proposal.md) separately on
2026-10-10. Both exact approval snapshots are retained. Reference-only records
and vectors do not establish runtime, migration or restart qualification.

#### Selected finite decisions and remaining proposals

On 2026-10-06 the user separately approved D1 and D2, then stated: "I don’t care
about breaking changes." Public API breaking changes are authorized for these
finite slices; do not add compatibility-only adapters. Preserve broader #792
tracking until its remaining profile decisions are resolved. No durable type or
layout decision is selected by that public API preference.

##### D1: ordinary relation aliases (#795)

Select public QuerySource::Aliased { source: Box<QuerySource>,
alias: String, column_aliases: Vec<String> }, with parsed quoted spelling retained
through the existing identifier authority. Keep existing Collection, Cte, Join,
Subquery and TableFunction serialization for unaliased inputs; an added enum
variant breaks external exhaustive matches, which the user has authorized.
Do not serialize alias wrappers into an existing durable catalog object without a
separate compatibility audit; approval here covers query AST only.

For this first contract, select ordinary Collection/Cte aliases, existing
self-joins, optional AS, and positional column-alias lists naming at most the source field count. Shorter
lists rename the corresponding prefix; remaining field names survive. Excess
names reject with 42P10. An alias hides the original relation qualifier; unqualified ambiguity
rejects, quoted names remain exact, and projected names/descriptors retain the
alias list. Source identity remains the underlying Collection for indexed/typed
specialization and cache/authorization context. Never emulate a base alias with a
Subquery wrapper that loses the physical Collection specialization.

Defer qualified-star r.* to a separately recorded AST/projection
contract rather than quietly accepting it. Existing wildcard behavior remains.
The owning #795 issue records the approved qualified-star exclusion. Finite #795
closure requires all other selected acceptance criteria, required gates, review
and merge readback; it does not require implementing the deferred qualified-star
contract and does not imply qualified-star support.

The rejected alternative was to freeze the public AST and defer aliases. The
selected wrapper keeps the source identity explicit instead of hiding aliases
inside a Subquery wrapper.

##### D2: parameterized pagination (#797)

Select public SelectStatement limit/offset as Option<Expr>, replacing the
historical Option<i64> fields. PR #858 merged this field shape on 2026-10-07
at `7977c848775ff4b776dbf40bda2bbb9893aac25d`; the merged commit is an ancestor
of the refreshed `55b52e26` baseline, and #797 is closed. The preparation notes
below are historical. Keep one authoritative bound expression per position
and reuse the existing expression AST. Do not introduce QueryBound or parallel
literal/expression fields solely to preserve historical Rust or serde shapes.

The user authorized the source and serialized-query API break. Literal 10 becomes
Some(Expr::IntegerLiteral(10)); LIMIT $1 becomes Some(Expr::Param(0)); explicit
NULL uses Some(Expr::Null); omitted/unbounded ALL uses None. Standard derived
Expr serialization applies (for example { "IntegerLiteral": 10 } or
{ "Param": 0 }, rather than a legacy bare integer or new {expr:...} wrapper).
Legacy serialized query AST inputs may fail; no legacy-only decoder is selected.
This changes the public query AST, not a stored catalog/type/layout contract.

The selected initial expression allowlist is: signed integer literals, NULL,
parentheses, parameters, checked + / - / * over admitted integer operands, and
explicit casts to current SMALLINT/INT/BIGINT. Infer a standalone untyped
LIMIT/OFFSET parameter as BIGINT (OID 20). Arithmetic selects an integer
signature from explicit casts, declared parameter types, or a concrete integer
sibling operand. A concrete `1` gives an otherwise unknown `$1 + 1` parameter
OID 23; an outer result cast does not select unknown argument operators. Do not
force unknown-plus-unknown operands to BIGINT. PostgreSQL 18.6 rejects
LIMIT $1 + $2 * $3 with 42725 (ambiguous operator), while explicitly BIGINT-cast
operands describe as three bigint parameters and execute. Validate explicit
declared/cast integer domains.
No floating arithmetic, division, functions, row references, subqueries,
aggregates, windows or volatile expressions in bounds. Evaluate once per
statement execution after parameter binding, before source opening, under the
existing admission/deadline controls. Checked overflow rejects; negative limits reject with 2201W, negative offsets with 2201X, integer overflow
with 22003; NULL limit is unbounded and NULL offset is zero; LIMIT ALL is unbounded.
All eligible plans, top-k, scans and cumulative portals consume that same bound.
A bound never allocates memory proportional to its magnitude.

This is a selected finite subset, not full compatibility proof.
The preparation oracle below pins the bare-parameter, explicitly cast arithmetic,
NULL/negative/overflow cases. At the time of the 2026-10-07 preparation record,
the local candidate had focused runtime evidence and remaining joint red/green,
descriptor, plan/portal and full-gate obligations recorded in [the integration
plan](alias-pagination-plan.md). PR #858 subsequently completed and merged the
selected #795/#797 bundle; that finite closure does not establish broad PostgreSQL
compatibility, complete #792/#793 coverage, or production readiness. No extended
type or persistent-layout approval is implied.

##### D3: extended contracts (#793)

Recommend keeping the current type/wire/layout authority unchanged and reviewing
the [per-family proposals](extended-type-decisions.md) separately after D1/D2.
No new NUMERIC, timestamp, JSONB, array, enum/composite, bit or vector identity is
approved by D1/D2. Families must not be removed from #791 merely to mark #792
complete. Snapshot/lock/transactional-DDL/routine decisions likewise need concrete
records; approving aliases and pagination does not authorize them.

#### Historical source refresh and current integration boundary

#851 merged at the source baseline above; [First Typed Pipeline Acceptance](typed-pipeline-acceptance.md) records completed #756/#757 behavior. The full
typed relational pipeline and Cassie-wide Production-ready gates remain open.
Legacy Planned labels in higher-level target documents must not imply that the
first scan/filter/projection slice is absent. This proposal defers support status
to Feature Support and adds no promotion.

Only documentation changed in the 2026-10-06 source refresh. Existing draft test/fixture work
in the original contract worktree was preserved and was not copied. Policy
validation and local-link/source-owner checks are recorded separately; no Cargo
build/test result is claimed for a documentation proposal. No issue closure,
PR publication or merge is authorized by this document alone. The 2026-10-07
joint-plan refresh is likewise documentation-only and describes the separate
local #797 runtime candidate and #795 implementation prerequisite as they stood
at preparation time. PR #852 later closed the #794 lexical prerequisite; PR #858
then completed and merged the selected joint #795/#797 bundle. The preparation
tests and validation below are historical evidence, not fresh validation of the
merged PR #858 head. Broader profile coverage and readiness gates remain open.

##### User selection record and publication boundary

D1 and D2 were separately approved on 2026-10-06. The subsequent explicit
preference permitting breaking changes supersedes compatibility-only QueryBound
serialization alternatives. Select the simpler Option<Expr> pagination AST.
No further public API approval is required for these finite changes.

The selected next minor publication boundary is 0.2.x. The #797 preparation
checkout retains 0.1.0 Cargo.toml/GitVersion.yml metadata; alignment is owned by
the first public scalar integration bundle and must be read back before publishing
these API changes. The existing manual publish
workflow and publish=false crate setting are integration facts, not authorization
to release. These documentation changes do not edit version metadata or publish.

Approvals do not authorize a new durable catalog/type/layout representation. If a
persisted parsed AST caller is discovered during implementation, produce its
concrete compatibility contract before changing that storage path. Neither the
finite selection nor API preference closes all of #792/#793.

#### Focused preparation oracle (2026-10-06)

A fresh isolated Homebrew PostgreSQL 18.6 process on Darwin arm64 executed
a generic alias/pagination statement corpus plus setup/version reads, with no Cassie runtime changes.
The process was stopped after evidence collection. Full SQL/results and executable
SHA256 are retained under target/contract-review in the refreshed worktree.
This is PostgreSQL semantic-oracle evidence for the listed cases. Cassie
implementation and finite issue closure were subsequently completed through PR
#858; this reduced oracle corpus does not establish broad dialect compatibility
or production readiness.

| Proposal boundary | Executed result / SQLSTATE |
| --- | --- |
| records r(first) over (id INT, score BIGINT) | Prefix rename succeeds; original score name survives, exact rows (1,10),(2,20),(3,30). |
| r(first,second) | Full two-column rename succeeds with those exact rows. |
| r(first,second,third) | Excess aliases reject 42P10. |
| records.id after FROM records r | Hidden original qualifier rejects 42P01. |
| Bare LIMIT $1 OFFSET $2 | PREPARE parameter_types is {bigint,bigint}; limit2/offset1 returns ids 2,3. |
| NULL/NULL and zero/zero bounds | All three ids for NULL/NULL; no rows for zero/zero. |
| Negative limit / offset | 2201W / 2201X respectively. |
| Uncast unknown arithmetic parameters | 42725, no successful PREPARE. |
| Explicitly BIGINT-cast arithmetic parameters | Three bigint descriptors; (1,1,1) returns ids 1,2; max-i64+1 rejects22003. |

Historical 2026-10-06 serde evidence was source-derived: public
QuerySource/SelectStatement and Expr used derived Serialize/Deserialize, bounds
were Option<i64>, and parser parameters used idx-1. This established the historical
integer JSON shape and zero-based Expr::Param index. The 2026-10-07 local #797
candidate used Option<Expr> and had focused serialization tests. PR #858 later
merged the selected #795/#797 bundle, and both issues are closed. No superseded
QueryBound proposal was implemented or tested; this historical record is not a
fresh exact-head validation of PR #858.
The user accepted breaking that historical query-AST shape. Implementation
must instead prove exact current Expr literal/parameter/NULL serialization and
execution behavior; no legacy literal-query round-trip requirement remains.

#### Historical proposal review record (2026-10-06)

Jev (jev-1.13.0) initially flagged pagination serialization approval gaps at
0.81. Source review pinned Option<i64>, zero-based parameters and the exact
legacy/new JSON alternatives; the focused remaining gap fell to 0.27. Root
review corrected alias lists to prefix renaming and prompted the fresh oracle
above, which also caught ambiguous uncast arithmetic parameters.

Final broad gap questions returned 0.33 for each alias/pagination proposal,
requiring focused probes. Pagination's focused choice selected no concrete
omission at 0.75. Alias's focused choice was inconclusive (arity 0.34, none 0.34,
qualification 0.31); the deterministic PostgreSQL/source probe resolves the named
arity issue: shorter lists rename a prefix and excess names reject 42P10. The
proposal kept qualified-star excluded and #795 open at that preparation stage.
The owning issue now records the approved exclusion and finite closure boundary
above. The integration owner
reviewed these concrete questions; the user subsequently selected D1/D2 and
authorized API breakage. Named runtime dependencies and full acceptance still
apply. Jev does not approve the
public AST, prove compatibility or close an acceptance criterion.

Documentation policy validation passed for 48 Markdown documents; local profile
links resolved (64 checked), and git diff --check passed. No runtime changes or
Cargo checks were performed for this documentation-only proposal.


### SQL-standard pagination spelling checkpoint (2026-10-16)

`FETCH FIRST` and `FETCH NEXT`, singular/plural `ROW`/`ROWS`, omitted count
(default 1), and `ONLY` lower to the existing checked integer limit expression.
An optional `ROW`/`ROWS` suffix after OFFSET uses the same standard count
syntax. Ordinary LIMIT/OFFSET expressions retain their existing grammar;
standard counts require parentheses around arithmetic, postfix casts or
nonliteral unary expressions. Single signed numeric constants remain valid.
LIMIT and FETCH share one duplicate-limit guard. OFFSET can precede or follow
FETCH; WHERE/GROUP/HAVING/ORDER clauses precede pagination.

Parenthesized set operands retain local bounds through the existing derived
source representation; the unparenthesized final FETCH/OFFSET tail applies to
the complete set query. Ordinary UNION CTE bodies remain nonrecursive. Known
integer row-dependent bound variables reject 42P10; source lookup first distinguishes
unknown columns (42703) from ambiguous columns (42702), and projection aliases
do not enter count scope. Typed `CassieError::AmbiguousColumn` retains that
semantic identity through SQLSTATE and telemetry.

These are finite PostgreSQL 18.6 witnesses over the existing integer foundation.
`WITH TIES` remains an open peer-aware execution obligation under #800/#821;
valid selected spellings reject 0A000. General floating/NUMERIC bound coercions,
functions/subqueries and complete recursive-CTE qualification remain pending.
The stored Value layout and query AST variants do not change; private compiled
plan epoch 8 invalidates earlier derived parsing/binding plans.


### Selected set quantifiers and VALUES branches (2026-10-16)

`SELECT ALL` preserves duplicates. Set operators accept explicit `DISTINCT`
(and their existing implicit DISTINCT default), while `INTERSECT ALL` consumes
one right occurrence for each matching left occurrence and `EXCEPT ALL`
subtracts right multiplicity from left multiplicity. NULL tuples compare equal
for this purpose. INTERSECT, including ALL, binds tighter than UNION/EXCEPT;
operators at the same precedence associate left to right.

Selected integer/text/NULL cases include standalone VALUES branches, CTE
materialization, empty inputs, text and multi-column keys, parenthesized
branch-local ordering/pagination, result ordinals and derived column aliases.
Global ordering uses exported result names after projection. New additive
`SetOperator::IntersectAll`/`ExceptAll` AST variants retain existing serialized
variant identities; compiled plans rebuild at epoch 9. No stored Value layout
or storage migration changes. Older binaries do not understand newly stored
query definitions using the new operators. Complete #800 acceptance, wider
set-type coercions, recursive CTE qualification, WITH TIES and ORDER BY USING
remain open.


### Selected typed VALUES scalar identities (2026-10-16)

VALUES now admits the existing DATE, TIME, TIMESTAMP, UUID and BYTEA scalar
casts alongside its integer, Boolean and character foundations. Selected valid
inputs retain their native column names, OIDs, sizes, typmods, NULL behavior
and text/binary payloads in standalone, derived, CTE and set-branch queries.
Unknown VALUES parameters infer these scalar input identities; supplied OIDs
retain their earlier contract. This is selected valid-input qualification,
not complete type-family or input-error acceptance.

SQL CHAR without a width still parses as CHAR(1). Internal bpchar with no
typmod means unbounded character input/result identity and advertises typmod
-1. Explicit CHAR casts truncate/pad by character count. Common-column
coercion preserves the first selected character-family identity, erasing width
when input typmods differ or an unknown input participates. Conversion from
CHAR to TEXT/VARCHAR drops trailing ASCII blanks; native comparison with
CHAR/VARCHAR/unknown operands ignores those blanks, while a typed TEXT operand
keeps its own blanks significant. Character keys retain declared identity
through projection and CTE handoffs so padded output does not change DISTINCT.

Character expansion is admitted before VALUES evaluation, constant folding
and projection; copied character descriptors remain charged to their owners.
Compiled plans rebuild at epoch 10. No new AST variant, public API surface, stored
Value layout, physical storage migration or second storage layer is introduced.

Complete #793/#800/#849 acceptance remains open, including broader type/input
errors, common-type conversions, functions/subqueries in VALUES and all
resource/lifecycle paths. Locale-aware comparison, sorting and keys remain
under [#796](https://github.com/cntryl/cassie/issues/796). The native oracle's
locale orders symbols differently from Cassie's current byte ordering; selected
Unicode identity witnesses therefore use independent integer ordering or ASCII
prefixes, with all native value bytes retained. No collation support is claimed.

### Selected explicit VARCHAR typmods (2026-10-16)

Explicit `VARCHAR(n)` casts now truncate by character count, including Unicode
and combining characters, across scalar projection, VALUES, derived sources,
CTEs, UNION ALL and supplied TEXT/VARCHAR/bpchar parameters. Selected text/binary
payloads and descriptors match PostgreSQL 18.6. NULL remains NULL, short input
is not padded, and CHAR/temporal/FLOAT8 source formatting precedes truncation.
Stored VARCHAR column assignment still rejects the selected nonblank overflows;
explicitly truncated INSERT SELECT output fits the column. General expressions
inside INSERT VALUES, complete assignment coercion/error identities, user-defined
casts and full #793/#911 acceptance remain pending. Private compiled-plan epoch
11 invalidates older results without a storage or public API migration.

### Selected integer/BYTEA casts (2026-10-16)

INT2/INT4/INT8 casts to BYTEA emit the declared number of big-endian two's
complement bytes. Reverse casts zero-pad shorter input, interpret full-width
input as signed, return zero for empty BYTEA and reject excess bytes (including
leading zeroes) with native 22003 range messages. NULL stays NULL. Existing
canonical hex BYTEA storage and standard scalar OIDs remain unchanged.

Declared source identity survives scalar/VALUES/CTE paths, mixed integer
promotion, stored columns and supplied parameter formats. Bind and Execute
check constants using supplied primitive values without putting values in
cached plans. Lazy CASE/COALESCE/AND/OR behavior admits only reached parameter
copies. Eligible parameter-free one-row VALUES CTEs reuse accounted source
propagation with canonical exported-name lookup; wider CTEs retain their prior
materialization path. Compiled plans rebuild at epoch 12.

The additive public IntegerByteaError/CassieError variant carries width/byte
count diagnostics and native range SQLSTATE/messages. No stored Value or AST
layout, storage migration, CI/release configuration or second storage layer is
introduced. Complete #793/#800/#816/#849 qualification remains open, including
BYTEA input/escape/session output, wider casts and functions, BIT/VARBIT,
network/UUID/hash/LSN families, complex/parameterized CTE planning and complete
resource/restart behavior. JSON/Vector parameters remain variable in this new
bound constant check; their broader qualification is not claimed.

### Selected BYTEA text input (2026-10-16)

Ordinary UTF-8 input, octal escapes 000–377, doubled backslashes and hex input
now canonicalize through one borrowed parser used by SQL casts and text Bind.
Hex whitespace is permitted between pairs, with native invalid-digit/odd-length
precedence. Malformed escapes report 22P02; malformed hex reports 22023, with
selected native ASCII diagnostics. Quoted errors occur during Parse, including
dead CASE arms; explicit TEXT constants and parameters reject at Bind and
stored TEXT rejects during execution. Failed Bind preserves statement reuse.

Both parameter input formats and output formats preserve BYTEA OID 17 and the
full 256-byte octal domain. Stored-column, cache/error/NULL/recovery, result
ownership, pre-cancellation and reached constant expansion probes accompany
this slice. Private compiled plans rebuild at epoch 13; canonical stored values
remain unchanged. Per-portal parameter retention, complete #849 qualification,
non-ASCII diagnostic-byte fidelity, session BYTEA output and encode/decode or
other binary functions remain pending. See [validation](testing/validation.md).

### Selected catalog read models (2026-10-16)

The b0d2b65e existing catalog and primitive codec foundations support this
#817 read-model increment. Table/view/sequence discovery reads real catalog
objects in the session database, including index presence and supported serial
allocation state. Ordinary/SERIAL/BIGSERIAL columns expose NO/NULL/NEVER identity
and generation flags. Twenty-six object families absent from current metadata
have native column-name shapes and empty rows. Dependency, rewrite, statistics,
operator-family and built-in text-search rows remain unavailable because they
require their actual semantic objects.

These experimental descriptors retain existing logical carriers; native domain
OIDs, NAME/reg* and opaque catalog codecs remain #793/#817 obligations. Ownership
is not recorded in current metadata, so new owner fields are NULL. Definitions
expose stored SQL, with native pretty-printing unqualified. Sequence allocation
probes use the supported SERIAL/BIGSERIAL omitted-field INSERT path. Standalone
nextval(), CREATE SEQUENCE AS/options and the legacy integer-default difference
remain #841 work; their existing native qualification tests stay red.

No persistent metadata, row format or type layout changes occur. Compiled plans
rebuild at epoch 14. Native shapes, selected values, lifecycle/database scope,
restart and empty text/binary exports are separately qualified in
[validation](testing/validation.md); no full catalog/security support is claimed.

Native hasindexes history after DROP INDEX is also pending under #817: PostgreSQL
retains the flag until maintenance, while the current read model reports current
index presence. Selected flag probes cover pre/post-create states. Do not treat
this staged read model as historical flag or complete relation-catalog parity.

### Catalog scalar helper increment (2026-10-17)

The Experimental #817 size helpers select `pg_size_bytes(text)` and
`pg_size_pretty(bigint)` using the existing primitive carriers, exact decimal
rounding and native error precedence. NUMERIC formatting and physical storage
size functions remain open. Encoding-name lookup selects native aliases and
NAME truncation for literals/stored TEXT/explicit TEXT parameters, without
implementing NAME's catalog/wire ABI or encoding conversion. Bare unknown NAME
parameters reject deterministically pending #793; #821 retains wider locale
qualification. See [catalog support](catalog-support.md) for exact boundaries.

### Runtime information increment (2026-10-17)

Experimental #817 runtime helpers report Cassie's actual recovery/JIT/NUMA
capabilities and Rust Unicode version over BOOL/TEXT. Compression reports NULL
for materialized ordinary SQL carriers, independently of Midge block codecs.
Loaded-module discovery supports a three-TEXT-column FROM relation with zero
rows; scalar RECORD projection remains pending under #793/#817. Typed primitive
parameters, native arity errors and untyped compression-parameter rejection are
selected; complete native catalog ABI and #849 resource qualification remain
open. See [catalog support](catalog-support.md) for exact boundaries.

### Option expansion increment (2026-10-17)

Experimental `FROM pg_options_to_table` uses selected one-dimensional logical
TEXT arrays and returns two actual TEXT columns. Literal constructors and TEXT
parameters converted to arrays are selected; direct native array parameters,
native array/RECORD ABI, bounds, dimensions, dynamic constructors and SQL array
assignment remain pending. Existing native descriptor and error probes do not
promote these absent cells. See [catalog support](catalog-support.md).

### Statement text increment (2026-10-17)

Experimental `current_query()` returns the original SQL using the existing TEXT
identity. Simple Query retains the entire message; prepared execution retains
Parse text across rebinding and portal pages. Embedded SQL, analyzed EXPLAIN and
view calls use an execution-local context. Result caching bypasses the helper.
This does not qualify the wider session/catalog surface or new NAME/OID types.
See [catalog support](catalog-support.md) and [validation](testing/validation.md).


Selected `pg_column_size(any)` support returns INT (OID 23). Explicit existing
fixed primitive casts use their declared widths, including SMALLINT/INTEGER/BIGINT
sharing the same Rust integer carrier. Unknown string literals include their
one-byte C-string terminator; explicit materialized TEXT/VARCHAR and BYTEA
literal or parameter casts include a four-byte uncompressed datum header.
UTF-8 byte lengths, NULLs, nested supported casts and text/binary wire formats
are covered against PostgreSQL 18.6. This describes selected SQL datums, not
Midge storage sizes. Direct typed parameters require explicit casts to preserve
identity. Unsupported stored varlena provenance, CHAR, NUMERIC, JSON, arrays,
vectors and unqualified expressions reject with `0A000` before evaluation;
these remain pending obligations under #793/#817. Untyped direct parameters
retain `42P18`. Query budget, cancellation and result ownership use existing
execution controls. Plan-cache cost model epoch is 19.


Experimental ten-batch increment 1: Selected text concatenation over existing scalar types.
Native wire witnesses and precise local acceptance are recorded in
[validation](testing/validation.md). Complete type and owner contracts remain pending.

Selected concatenation returns TEXT OID 25 and propagates NULL. TEXT/VARCHAR
and unknown string inputs may concatenate with selected integer, FLOAT8 and
Boolean scalars; arithmetic binds first. Parameters retain declared identities.
Closed INSERT concatenation uses the existing scalar evaluator, and the original
mutation witness passes unchanged. CHAR padding, BYTEA, array, JSON, temporal
and other overloads remain pending; this increment does not infer their layouts.
The old parser-only rejection list now retains only still-pending operators.
Plan-cache epoch: 20. Jev's 0.75 overload concern prompted 16 native/Rust
parameter-format probes; final review selected verification at 0.94.


Experimental ten-batch increment 2: Power and integer remainder over existing scalar types.
Native wire witnesses and precise local acceptance are recorded in
[validation](testing/validation.md). Complete type and owner contracts remain pending.

Selected power accepts existing integer and explicitly selected FLOAT8 inputs,
uses left associativity and PostgreSQL unary-sign precedence, and returns FLOAT8
(OID 701). Raw decimal literals and NUMERIC casts remain pending rather than
borrowing FLOAT8 identity. Integer remainder preserves selected int2/int4/int8
widths, signed remainder, NULL and division-by-zero semantics; minimum integer
remainder by -1 returns zero. Compound operator names with adjacent signs are
not split into these selected operators. Eleven native FLOAT8 special witnesses
cover NaN/infinity and domain errors. The negative-infinity fractional-power
gap and three raw-decimal provenance gaps were red before correction. All 58
cumulative wire tests pass; 39 are new in this batch, plus one parser boundary
test. Parser failures are 67 with no new identity. Final checks follow only an
equivalent match-arm consolidation after the parser comparison. Cache epoch 21.
