# Proposed finite SQL dialect profile

This proposed contract owns [#792](https://github.com/cntryl/cassie/issues/792) under [#791](https://github.com/cntryl/cassie/issues/791). Historical source baseline: `4a09c41a3670dad3a4c4c2b7ef7583327c37ab6b` (2026-10-06 refresh). Child issue source witnesses retain their original baseline `9ff42bc05c374f478a12a29caf7537268215e665`; those are historical mappings, not current execution. It inventories every one of the 58 new child owners and their existing prerequisites. Publication of this document does not approve a new public or durable contract, close an issue, or qualify runtime behavior. [Feature Support](feature-support.md) remains the sole current support owner; [Production Readiness](production-readiness.md) owns readiness.

## Boundaries and approval

Cassie remains single-node, Midge remains the direct storage and recovery owner, pgwire remains primary, and REST remains secondary and administrative. Distributed SQL, replication, fleet coordination and arbitrary PostgreSQL extension loading remain outside this profile. Existing `cassie-midge-layout-v2` and accepted current type identities remain unchanged until an explicit successor decision.

[Query Engine Target 1](query-engine-target.md) remains the finite current-type execution target. Its exclusions are preserved for that target. The rows below propose later work for selected PostgreSQL-shaped syntax, exact types, routine/trigger execution, locking, transactional DDL and a selected vector identity profile. They do not retroactively remove Target 1 exclusions. Full PostgreSQL/server/catalog/extension parity and blanket OLTP certification remain unclaimed.

Approval is per finite family. The user selected the finite relation-alias AST/namespace and pagination expression contracts below on 2026-10-06 and explicitly authorized public API breaking changes. Existing-type syntax and scalar work may be approved independently of unresolved durable-type and transaction decisions. Native issue prerequisites must record this finite selection before dependent implementation; the full #792/#793 programs remain open. No wildcard “all PostgreSQL syntax” acceptance is permitted. Each row's implementation issue must pin the precise forms, type signatures, errors and exclusions before coding.

## First wave: selected carveouts and proposed expansions

The #794/#802 rows record selected existing-contract carveouts. The finite #795/#797 public AST changes were selected by the user on 2026-10-06. None selects a stored type, durable layout, public batch interface or stronger transaction guarantee.

| Owner | Finite selection | Required boundaries |
| --- | --- | --- |
| #794 (selected finite slice) | Compact/spaced existing comparison operators =, !=, <>, <, >, <=, >= and arithmetic +, -, *, /; comments and quoted literals retain lexical boundaries. | Longest valid operator match; numeric exponent/sign boundaries; quoted operator text is inert; malformed tokens reject deterministically. No new operator/cast/type syntax, public AST, wire identity or stored format. Expanded casts remain #796-owned. |
| #795 after #794 | Explicit/implicit ordinary base-relation aliases, schema qualification, alias column lists and self-joins using existing join forms. | Alias hides original relation name; ambiguous references reject; quoted spelling remains exact; output labels and Describe/Execute agree; derived/CTE scopes remain intact. |
| #802 (selected finite slice) | NULLIF(a,b), GREATEST(a,b,...) and LEAST(a,b,...) over selected existing SMALLINT/INT/BIGINT/FLOAT/TEXT/BOOLEAN families. | PostgreSQL 18 coercion/result typing and NULL laws: NULLIF returns NULL on equality, otherwise first argument; extrema ignore NULL and return NULL if all inputs are NULL. NULLIF operator-selected first-operand return type, extrema common type and unknown/all-NULL descriptors require an explicit compatibility matrix; ARRAY/JSON/VECTOR are excluded; all-integer comparisons remain exact above 2^53; admitted mixed integer/FLOAT inputs follow PostgreSQL promoted FLOAT rounding. Temporal/CHAR/VARCHAR/ARRAY/JSON/VECTOR are excluded from this selected slice. Existing error priority and cancellation remain. |
| #797 | LIMIT/OFFSET integer literals, typed bound integer parameters and selected side-effect-free scalar integer expressions; LIMIT ALL and NULL. | NULL LIMIT means unbounded; NULL OFFSET means zero; negative and out-of-range reject; zero LIMIT returns no rows; checked integer conversion; stable ordering fixtures; scan/top-k/portal budgets use the same admitted bound. Correlated, aggregate, window and subquery bounds remain excluded until separately selected. |

Prepared parameter Describe must advertise the selected type without performing writes or publishing output. Admission, cancellation, deadline and result-cap failures preserve existing statement/transaction cleanup. NULLIF/extrema are not ordinary eager function aliases where PostgreSQL typing or evaluation semantics would differ; the chosen implementation must reproduce the oracle for admitted forms.

### First-wave decision status

- #794: the bounded lexical carveout is selected in the issue; PR #852 was squash merged on 2026-10-07 to `3b03dd4db34ac88f533501d0484ecdc3cf223a2e`; #794 is closed and the merged source was read back. Scalar dollar-quoted literals, repeated unary-sign chains beyond a single signed numeric literal and chained/expanded postfix casts remain excluded. Protecting dollar-quoted text lexically does not admit it as a scalar literal. No broad #792 approval follows.
- #802: the selected finite existing-type coercion, unknown/all-NULL TEXT result, explicitly cast parameters and runtime eager-operand rules are recorded in #791/#802. They are not awaiting a new broad type approval. The now-merged native #794 prerequisite was required because compact function-argument operator suffixes can be truncated before evaluation; the 2026-10-06 issue comment records that witness. Inner COALESCE promotion before arithmetic is separately tracked by #850 and gates #761/#848.
- #795: the user selected the public alias wrapper, ordinary alias/prefix-list namespace rules and qualified-star deferral below. #794 remains an implementation prerequisite; full #795 acceptance is not closed by this selection.
- #797: the user selected the finite integer/NULL/parameter/error rules below and authorized clean public AST changes. The 2026-10-07 local candidate uses Option<Expr> bounds rather than a compatibility-only QueryBound wrapper. This is candidate implementation with completed local validation, not merged-main support. The [joint alias/pagination closeout](alias-pagination-plan.md) records full-suite and equivalent-refinement provenance and remaining publication checks.

Scalar oracle evidence: the preparation lane retained `target/dialect-scalars-evidence/oracle-results.txt`, `oracle.sql` and `postgres.sha256` in its isolated worktree. The local PostgreSQL 18.6 Homebrew executable is aarch64-apple-darwin25.6.0, Apple clang 21.0.0, SHA256 `db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`. Runtime column-divisor fixtures establish eager operand evaluation for NULLIF and extrema; a constant NULL first argument can be folded by PostgreSQL and is not a runtime laziness contract. Supplementary NULL-column fixtures preserve that distinction. Mixed BIGINT/FLOAT comparison follows the selected PostgreSQL promotion rather than claiming exact mixed-domain equality. This evidence does not approve #802 by itself.

## Semantic oracle and evidence

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

## Complete ownership matrix

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

## Proposed prerequisite refinement

The integration owner explicitly amended #794 and removed its broad #792 blocker before implementation, selecting the finite existing-operator lexical slice: it preserves selected #752/#753/#432/#755 types, NULL/error/resource/wire contracts, and adds no operator family, public AST or storage identity. Expanded operator families remain profile-gated.

The integration owner selected #802 finite SMALLINT/INT/BIGINT/FLOAT/TEXT/BOOLEAN signatures, PostgreSQL 18.6 coercion/NULL/eager-evaluation cases and explicitly cast parameter inputs, then removed its broad #792 blocker before implementation. New type families or unresolved coercion forms remain #792/#793-gated. The selected #802 slice reuses closed #752/#753/#432/#755 contracts. The #794 selection does not close or approve the full #792 profile. #794 is now merged and closed after the compact-argument lexical finding. #795 has the finite public alias AST/namespace contract selected below, and its #794 runtime prerequisite is now merged and closed.

## Delivery and ownership

The integration owner owns contract review, shared AST/catalog/storage decisions, the dependency graph, exact-head review and serial publication/merge. Persistent sibling worktrees have separate target directories and server ports. Current #851 merged scan/filter/projection and portal qualification before this refresh; #756/#757 are closed and must not be rebuilt as missing foundation. Later #758/#759/#760 typed aggregates, joins and ordering/set/window kernels remain open. First-wave lanes are syntax/binding (#794 then #795), scalar expressions (#802) and pagination (#797). Parser changes are syntax-owned; function metadata/evaluator changes are scalar-owned; pagination isolates its helper and integration reviews shared query AST changes. Null-safe predicate grammar #798 follows the syntax merge.

After the first wave, aggregate modifiers/Boolean/collection/statistical operators, JSON expressions and approved extended types can proceed in separate lanes only after their native prerequisites close. Array/table expansion follows its type/source prerequisites. Routines, triggers, isolation, transactional DDL and partitions remain blocked on selected lifecycle and persistence decisions. Each worker delivers one coherent bundle; integration publishes one PR at a time, verifies its final head and squash-merges, then remaining workers rebase and rerun affected checks.

## Unresolved contract decisions

[#793](https://github.com/cntryl/cassie/issues/793) owns the [extended-type decision inventory](extended-type-decisions.md). The decisions below remain unresolved and block affected runtime families. They must be approved as concrete contracts; this document does not choose their durable or public outcome.

| Decision | Required resolution | Blocked owners |
| --- | --- | --- |
| Extended types/ABI | Values, names/OIDs/modifiers, codecs, operators, durable representation and migration/recreate policy. | #793 and its typed dependents. |
| Temporal scope | Time-zone database/version, calendar/month arithmetic, DST ambiguity and SQL clock lifetimes. | #814/#815. |
| Regex/text collation | Engine/Unicode version, collation/case rules, pattern budget and bounded execution. | #799/#803. |
| Procedural language | Finite language/control flow, dynamic SQL binding, recursion/resource bounds and trusted-language security. | #830/#832/#835/#836/#837. |
| Locks/isolation | Snapshot level, row identity/conflicts, deadlock policy, lock lifecycle and cancellation. | #819/#820/#822. |
| Transactional catalog changes | Atomic schema publication, durable recovery, catalog/plan invalidation and concurrent readers. | #833/#837/#838/#839/#840. |
| Extension identity | Allowlisted built-in identities and explicit compatibility subset; no arbitrary plugin ABI. | #843/#844. |
| Final finite corpus | Concrete signatures/forms per row and client versions; unresolved forms stay excluded. | #847/#848/#849. |

Issue #792 remains open for the broad profile; user selection applies only to D1/D2 and existing named carveouts. It does not complete #793 or any runtime child.

## Selected finite decisions and remaining proposals

On 2026-10-06 the user separately approved D1 and D2, then stated: "I don’t care
about breaking changes." Public API breaking changes are authorized for these
finite slices; do not add compatibility-only adapters. Preserve broader #792
tracking until its remaining profile decisions are resolved. No durable type or
layout decision is selected by that public API preference.

### D1: ordinary relation aliases (#795)

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

### D2: parameterized pagination (#797)

Select public SelectStatement limit/offset as Option<Expr>, replacing the
historical Option<i64> fields. The 2026-10-07 local candidate implements this
field shape; merged support and full validation remain pending. Keep one authoritative bound expression per position
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
LIMIT/OFFSET parameter as BIGINT (OID 20). Parameters inside arithmetic must
have explicit integer casts or declared integer parameter types; do not
conveniently force every unknown operand to BIGINT. PostgreSQL 18.6 rejects
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
NULL/negative/overflow cases. The local candidate has focused runtime evidence,
with remaining joint red/green, descriptor, plan/portal and full-gate obligations
recorded in [the integration plan](alias-pagination-plan.md). No extended
type or persistent-layout approval is implied.

### D3: extended contracts (#793)

Recommend keeping the current type/wire/layout authority unchanged and reviewing
the [per-family proposals](extended-type-decisions.md) separately after D1/D2.
No new NUMERIC, timestamp, JSONB, array, enum/composite, bit or vector identity is
approved by D1/D2. Families must not be removed from #791 merely to mark #792
complete. Snapshot/lock/transactional-DDL/routine decisions likewise need concrete
records; approving aliases and pagination does not authorize them.

## Historical source refresh and current integration boundary

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
joint-plan refresh is likewise documentation-only, but it describes the separate
local #797 runtime candidate and the #795 implementation prerequisite as it stood
at preparation time. Subsequent #852 merge readback permitted the selected joint
#795/#797 runtime candidate; its focused tests do not substitute for pending
complete gates or merged support. Historical
proposal validation below is not fresh validation of that candidate.

### User selection record and publication boundary

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

## Focused preparation oracle (2026-10-06)

A fresh isolated Homebrew PostgreSQL 18.6 process on Darwin arm64 executed
a generic alias/pagination statement corpus plus setup/version reads, with no Cassie runtime changes.
The process was stopped after evidence collection. Full SQL/results and executable
SHA256 are retained under target/contract-review in the refreshed worktree.
This qualifies these proposal witnesses only, not #795/#797 implementation.

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
candidate now uses Option<Expr> and has focused serialization tests; the joint
full gates and merged support remain pending. No superseded QueryBound proposal
was implemented or tested.
The user accepted breaking that historical query-AST shape. Implementation
must instead prove exact current Expr literal/parameter/NULL serialization and
execution behavior; no legacy literal-query round-trip requirement remains.

## Historical proposal review record (2026-10-06)

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
