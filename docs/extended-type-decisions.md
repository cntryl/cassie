# Extended SQL type decision inventory

This is an unresolved decision inventory for [#793](https://github.com/cntryl/cassie/issues/793), following the [proposed dialect profile](sql-dialect-profile.md). It selects no persistent format or public wire identity. The [current finite type contract](type-contract.md) remains authoritative until a successor decision is approved and implemented.

The current source baseline is `4a09c41a3670dad3a4c4c2b7ef7583327c37ab6b`; historical child witnesses use `9ff42bc05c374f478a12a29caf7537268215e665`. Current values, declared types and codecs are owned by `src/types/value.rs`, `src/types/schema.rs`, `src/midge/row_blob/encoding.rs` and `src/pgwire/`. Source mappings below are planning evidence, not fresh execution or new support claims.

## Decisions required before implementation

| Family / owner | Existing boundary | Unresolved concrete decision |
| --- | --- | --- |
| Exact NUMERIC/DECIMAL #812 | Current declarations alias float8; text numeric input adapters do not establish exact decimal storage/results. | Precision/scale bounds, arbitrary versus fixed precision, rounding/overflow, NaN/infinity policy, exact mixed arithmetic/comparison, OID 1700 result/array codecs, durable tag/representation and migration/recreate policy. |
| FLOAT4 #793 | OID 700 is input adaptation to f64, without a logical float4 result family. | Distinct logical f32 representation, rounding/finite policy, result identity/codecs, promotion rules and durable representation. |
| TIMESTAMPTZ/INTERVAL #814 | One normalized TIMESTAMP result family; no selected separate timezone or calendar interval type. | Instant versus local time identity, timezone database/version, DST ambiguity, interval month/day/microsecond representation, typmods, overflow, clock lifetimes, codecs and storage migration. |
| JSONB #809/#810/#829 | JSON result OID 114 and document carrier; JSONB ABI not selected. | JSON versus JSONB equality/order/hash, duplicate-key and number rules, canonicalization, containment/path operators, OID 3802 and array codecs, durable preservation versus replacement. |
| Standard arrays #807/#808 | One-dimensional arrays, private durable OID formula and selected element exclusions. | Standard wire OIDs versus current identities, dimensions/lower bounds, SQL NULL versus JSON document-null elements, element typmods, maximum sizes, codec compatibility and existing stored-array transition. |
| Named enum #827 | No selected named enum type identity. | Stable name/member identity, order/renames/additions, per-database OID allocation, codecs, dependency tracking and durable evolution. |
| Composite/record/domain #828 | No selected named composite/domain result or durable family. | Named versus anonymous record identity, field order/names/types, constraints/casts, recursive references, codecs, schema evolution and recovery. |
| BIT/VARBIT #816 | BYTEA is existing raw-byte type; bit strings are not that contract. | Fixed/variable length, bit order/padding, conversions, operators and widths, OIDs/codecs, durable tags and limits. |
| Selected vector profile #844 | Current finite f32 VECTOR(n), custom dimensional OIDs; pgvector ABI excluded from Target 1. | Extension name/version allowlist, scalar/vector-array identities, unconstrained versus dimensioned casts, operators/operator classes, binary ABI and existing data/identity transition. |

## Per-family decision record

Every selected family must record all of the following before runtime work starts:

1. Accepted literal/declaration/cast spellings and explicit excluded forms.
2. Logical domain, NULL identity, equality/order/hash, mixed-type coercion and error laws.
3. Names, OIDs, typlen/typmod, text/binary codecs, parameter Describe/Bind and output Describe/Execute identity.
4. Durable row/field encoding, format/version ownership, recoverability and migration or explicit recreate policy.
5. Catalog/dependency lifecycle, schema changes and prepared-plan/cache invalidation.
6. Checked allocation and value bounds, cancellation/deadline behavior and malformed-input rejection before publication.
7. Pinned PostgreSQL oracle fixtures where parity is selected, independent exact-value/storage witnesses, cross-interface and restart acceptance.
8. Approved decision provenance and exact blocker/implementation owners.

OID replacement cannot be treated as parser cleanup; durable tags cannot be selected opportunistically to make a test pass. A successor contract must describe existing-data behavior and protocol callers. If migration is excluded, record the operational recreate boundary explicitly before implementation.

## First-wave decisions that do not require new durable types

Syntax/alias and integer pagination work can preserve the current type universe. The #802 finite type-resolution decision within that universe is already selected in #791/#802: NULLIF uses the admitted equality operator's first-input/result contract; GREATEST/LEAST use one common ordered result type. Cross-width integer, integer/float, unknown/all-NULL and quoted temporal literal cases need exact oracle descriptors. ARRAY, JSON and VECTOR comparison is excluded from the proposed first wave. No exact NUMERIC or new temporal family is implied.

Native #794 remains the #802 lexical integration blocker; #850 separately tracks existing COALESCE numeric promotion. No broad #792/#793 approval is inferred from those carveouts. The integration owner should review extensions to those finite cases separately from this extended-type inventory. An unresolved descriptor/coercion case remains excluded or blocked until its decision is recorded; it must not silently inherit a registry helper's convenience type.

## Recommended finite proposals (all pending approval)

These options make #793 review concrete; they do not select bytes, public OIDs or
migration policy automatically. Each row still requires its complete eight-field
record above and the named focused compatibility/restart oracle before runtime.

| Family | Recommended first selection | Explicit remaining gate |
| --- | --- | --- |
| NUMERIC #812 | Finite exact decimal domain, initially explicit NUMERIC(p,s) with p <= 38 and 0 <= s <= p; checked operations and oracle-defined rounding. Keep historical float8 alias interpretation until a separately approved successor declaration contract. | Unconstrained NUMERIC, exponent/negative scale, mixed FLOAT behavior, wire OID 1700 and existing schema/data interpretation remain unresolved. A new durable tag requires approved version/recreate or migration policy. |
| FLOAT4 | Distinct finite f32 logical result with PostgreSQL OID 700, explicit rounding on conversion and widening to current FLOAT only where selected. | Durable tag, nonfinite SQL versus storage policy, and exact mixed coercion table must be reviewed. OID 700 input adaptation alone is insufficient. |
| TIMESTAMPTZ/INTERVAL #814 | TIMESTAMPTZ as exact signed UTC microseconds; INTERVAL as distinct months/days/microseconds, never collapse months to fixed seconds. Begin with UTC and explicit fixed-offset inputs. | IANA-zone/DST rules require a pinned timezone source/version before inclusion; current TIMESTAMP remains its documented separate contract. New codecs/tags and transition require approval. |
| JSONB #809/#810 | Distinct JSONB identity with PostgreSQL-compatible versioned wire framing, last duplicate key wins, and selected numeric/equality/containment laws. Preserve current JSON OID 114 identity. | Exact number domain/canonicalization, hash/order, object duplicate semantics and new durable representation must be reviewed before selecting OID 3802 outputs. |
| Arrays #807/#808 | First admit only current one-dimensional/lower-bound-one shape with standard OIDs for registered scalar elements; checked element identity/NULL validation. | Public OID change and backward compatibility must be approved. Multidimensional/custom bounds and direct JSON document-null element representation stay blocked, not silently flattened. |
| Enums #827 | Per-database named identity; declared member order defines comparison; immutable persisted member identity through rename/add. | Collision-free stable OID allocation, pgwire codecs, dependency invalidation and transactional enum evolution must be specified. |
| Composite/record/domain #828 | Named immutable-version record descriptors and explicit positional field codecs; domain constraints enforced at casts and assignments. | Anonymous-record wire identity, recursion, schema evolution, null versus null-field record laws and durable recovery representation require decisions. |
| BIT/VARBIT #816 | Distinct bit length plus packed most-significant-bit-first payload, checked unused padding, finite statement/value bounds and explicit width errors. | OIDs 1560/1562, typmod/cast rules and durable tags require approval; do not reuse BYTEA semantics. |
| Vector #844 | Explicit finite built-in vector compatibility profile; retain current finite f32 data domain while deciding extension metadata and dimensional/unconstrained wire identities separately. | No pgvector ABI/version/OID promise until exact compatibility matrix and existing-data transition are approved. halfvec/sparsevec/quantization remain excluded. |

Recommend reviewing NUMERIC and temporal contracts independently; their distinct
semantic and migration decisions do not need to block existing-type aliases or
pagination. Broader prerequisites can be refined only through reviewed issue/doc
updates, never an implementation convenience.

## Persistent transition decision

The smallest common durable question is whether a new type/layout revision must
read existing cassie-midge-layout-v2 data. Recommend retaining v2 without new type
tags for current slices, then proposing a new explicitly versioned layout with
verified export/recreate/reimport as the initial upgrade path. This is a proposal
only: export must preserve exact old data/identity, operator rollback and restart
proofs must be specified, and existing deployments must explicitly accept that
boundary. An in-place mixed-version reader/migration would be a different,
substantially larger scope and cannot be inferred from #793.

## Finite public API selection refresh (2026-10-06)

The user separately approved ordinary aliases and bounded expression pagination,
then explicitly authorized public API breaking changes. D1 selects an Aliased
query-source wrapper; D2 selects Option<Expr> bounds with the approved finite
integer/NULL/parameter/error semantics. Existing literal-query serde compatibility
is not a requirement; no compatibility-only QueryBound wrapper is selected.
This preference does not approve new stored tags, wire type identities, catalog
recovery changes or layout migration. All extended family records in this
document remain pending, and #793 remains open.
