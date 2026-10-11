# PostgreSQL 18 type family records

These are #793's implementation contracts for the full selected PostgreSQL
18.6 target, not evidence that Cassie implements them. Earlier 38-digit,
one-dimensional, fixed-offset and finite-only scalar selections are intermediate
slices. PostgreSQL's admissible domains, errors and missing operations define
the final boundary. The source archive SHA256 is
`555610c24d53e4316da5b7d3fc25c279d96856d5e0e23ee308c328c5fa881d9f`.

Each record has the required eight fields. Built-in OIDs, array OIDs, typlen,
category/preference/collation, input/output/receive/send/typmod routine identities
come from the accompanying immutable 193-type registry. Its `pg_type.dat`,
`pg_proc.dat`, `pg_cast.dat`, `pg_operator.dat`, `pg_opclass.dat`,
`pg_opfamily.dat`, `pg_amop.dat` and `pg_amproc.dat` authorities are normative
by hash. Function existence does not imply externally constructible values.
The owning PostgreSQL source routines define all lexical edge cases and errors;
the summaries below select those routines' semantics, not a new approximation.

The exact [catalog/export schemas](postgresql18-catalog-export-proposal.md)
were explicitly selected separately on 2026-10-10. Their retained approval
snapshot fixes the reviewed bytes. Selection authorizes affected implementation
after recorded prerequisites, without claiming runtime qualification. #793
remains open for staged owner records and actual acceptance.

## Shared fields applied to every record

1. **Syntax.** PostgreSQL 18.6 declaration/literal/constructor/array/qualified
   type and cast grammar, aliases and typmod routines. Grammar owner #796, row
   grammar #800. A form legal only in a routine/signature is not a stored type.
2. **Value laws.** SQL NULL is separate from every non-NULL value, including
   JSON null, empty collections and records with NULL fields. Resolve operations
   from the actual catalog signatures and type/collation laws. Missing equality,
   order or hash support remains PostgreSQL's error; no bytewise fallback.
3. **Wire.** PostgreSQL built-in OIDs and typmods; selected named stable OIDs.
   Describe/Bind/Execute share one registry. Use the actual text/binary I/O
   function and SQLSTATE. Text is transcoded through the selected client/server
   encoding; durable text preserves the database-encoding bytes. SQL_ASCII is
   uninterpreted non-ASCII bytes, not forcibly decoded as UTF-8. Wire typlen is catalog typlen, not payload
   length. NULL parameter length -1 carries no payload. Infer unknown parameters
   through the PostgreSQL resolver, never the first bound value.
4. **Durability.** Approved CRB3 framing/versioned codec tags, immutable enclosing
   descriptors and separate-store export/recreate/reimport. Wire receive can
   accept a noncanonical input that PostgreSQL normalizes; persist the selected
   send result after validation, rather than rejecting a legal wire input merely
   because its bytes differ. No heap/varlena or Rust enum ABI is durable.
5. **Lifecycle.** Transactional catalog root publication, retained old descriptor
   revisions, dependencies/privileges and generation/context-bound plan/cache
   invalidation. Rename preserves identities; drop/recreate never reuses a
   committed named identity. Domains/enums/collations and container subtypes are
   part of the dependency set. Recovery never skips undecodable authoritative
   state. Exact catalog/export schemas are separately recorded.
6. **Resources.** Check every length/product/offset/typmod/range before allocating.
   Reserve actual retained and temporary memory through shared query controls
   before ownership; use checked arithmetic and bounded cancellation checks.
   Logical PostgreSQL value limits are distinct from configurable query budgets.
   Budget exhaustion is a truthful resource error, not truncation or an exclusion.
   Primitive bounds and resource/cancellation laws are in the resource record.
7. **Evidence.** Pinned reference fixtures include raw simple/extended text/binary
   bytes, OIDs/typmods, parameter descriptors, command tags and SQLSTATE.
   Independent durable goldens/malformed controls qualify encoders/decoders;
   each owner must add scalar/typed/index/overlay/wire/rollback/restart and upgrade
   witnesses before support promotion. Reference-only success is not Cassie PASS.
8. **Provenance/owners.** Full PostgreSQL semantics were selected by the user plan;
   common v3 and catalog/export directions explicitly approved 2026-10-10.
   #793 owns these records,
   #904 shared implementation, #848 complete differential qualification, #849
   resource/restart evidence. Each owner below qualifies interaction with existing
   expressions/operators; later integration obligations do not gate unrelated
   primitive codecs. Unselected persistent additions remain gated explicitly.

The following numbered family details specialize these shared fields; no field
is left to an implementation convenience.

## Boolean and signed integers — #904

1. bool/boolean, int2/smallint, int4/integer/int, int8/bigint and PostgreSQL
   literals/casts; serial declarations belong to #841 and create integer state.
2. Boolean three-valued logic; exact signed 16/32/64-bit domains, checked integer
   operations. PostgreSQL mixed-signature resolution and overflow/division errors.
3. OIDs 16/21/23/20; binary Boolean one byte, integers 2/4/8 bytes network order.
   Boolean receive follows PostgreSQL's nonzero rule; send emits 0 or 1.
4. Tag 1/version 1, exact send payload. Integer widths remain distinct types.
5. Immutable built-in descriptors and standard scalar-array dependencies.
6. Fixed width and checked arithmetic, no widening to FLOAT to avoid overflow.
7. Signed extrema, true/false/NULL, noncanonical Boolean input, overflow and
   aggregate/scalar mixed-width descriptors on every applicable path.
8. Shared provenance; #904 primitive laws, #813 numeric functions, #804 aggregates.

## NUMERIC/DECIMAL — #812/#813

1. Both names, unconstrained NUMERIC, NUMERIC(p), NUMERIC(p,s), exponent literals,
   explicit precision 1–1000 and scale -1000–1000, including scale greater than p.
2. Exact decimal with display scale; PostgreSQL unconstrained digit bounds and
   operation-specific result-scale/rounding. Include NaN and signed infinities;
   NaNs compare equal and after other numbers. Constrained infinities error.
   Typmod rounding precedes precision overflow; decimal ties round away from zero.
3. OID 1700, array 1231, typlen -1. Typmod is
   `((p << 16) | (s & 0x7ff)) + 4`; -1 is unconstrained. Binary is u16 digit count,
   i16 base-10000 weight, u16 sign, u16 display scale, then u16 digits <10000.
   Finite sign 0x0000/0x4000; NaN 0xc000, infinities 0xd000/0xf000. Exact canonical
   special-value headers follow `numeric_send`, including its scale field.
4. Tag 1/version 1, `numeric_send` bytes; never f64, JSON number or Rust decimal
   with a fixed 38-digit final limit. Existing v2 NUMERIC aliases export FLOAT8.
5. Typmod changes publish descriptors and enforce new assignments/casts using
   PostgreSQL laws; old stored descriptors retain their original interpretation.
6. PostgreSQL unconstrained bounds (131072 digits before, 16383 after decimal),
   checked base-10000 operations and accounted work buffers; cancel long parsing,
   arithmetic, comparison and aggregate work without publishing partial mutation.
7. Fixtures cover >38 digits, both extreme unconstrained scales, negative/extended
   scale, rounding/overflow, specials, mixed float/integer resolution, wire and
   durable exact bytes, invalid signs/digits/scales, restart and upgrade alias laws.
8. Shared provenance; #812 representation/arithmetic, #813 scalar functions,
   #904 resolver/codec dispatch, aggregate owners qualify transition states.

## FLOAT4/FLOAT8 — #793/#904/#813

1. real/float4, double precision/float8, FLOAT and FLOAT(p), p 1–53 mapped by
   PostgreSQL to f32 for p<=24 and f64 for p>=25; all selected special spellings.
2. Distinct IEEE binary32/binary64 domains, signed zero, infinities and NaNs.
   PostgreSQL comparison/equality/hash, rounding, overflow and underflow laws;
   NaN payloads and signed-zero bits are preserved by raw scalar send/receive.
   SQL equality/hash canonicalize as PostgreSQL requires, not raw bits.
3. OIDs 700/701, arrays 1021/1022, typlen 4/8, typmod -1. Network IEEE payloads.
   Text uses PostgreSQL shortest precise output with `extra_float_digits`, including
   older requested precision; do not use generic Rust display as the contract.
4. Tag 1/version 1, exact send payload. Arithmetic-produced special bits follow
   the pinned PostgreSQL operation; input round trips retain received bit patterns.
5. Distinct descriptors; conversions/constant folding/cached plans preserve f32
   rounding and the selected operand/result type.
6. Fixed scalar width; parse/format temporary buffers and aggregate transitions
   remain accounted. No finite-only final scalar policy.
7. f32/f64 0.1, signed zero, noncanonical NaN payload, specials, underflow/overflow,
   extra_float_digits and mixed-type parameter/aggregate descriptors.
8. Shared provenance; #904 primitive representation, #813 mathematical functions.

## Character strings, bytea and UUID — #803/#816/#904

1. text, varchar/character varying, bpchar/character/char, quoted internal "char",
   name, bytea and uuid; PostgreSQL escape/Unicode/binary/UUID spellings.
2. Text respects encoding, typmod character counts and collation; bpchar pads and
   ignores trailing spaces for its comparison laws. Internal "char" is one byte;
   name follows NAMEDATALEN 64 and multibyte-safe truncation. bytea is arbitrary
   bytes including zero; uuid is 128 bits. Text NUL/invalid encoding errors match
   PostgreSQL. No implicit JSON carrier or generic string ordering.
3. OIDs 25/1043/1042/18/19/17/2950 and registry arrays. varchar/bpchar typmod=n+4,
   -1 where unspecified under PostgreSQL syntax. Binary text/name omit C terminator;
   char one byte; bytea raw; UUID 16 network-order bytes. Settings govern text output.
4. Tag 1/version 1, actual send payload with client_encoding fixed to the database encoding for
   durable serialization; record the database encoding. Preserve
   bpchar padding and bytea identity. UUID has no Rust UUID serialization ABI.
5. Collation/provider version and typmod are descriptor/cache dependencies.
6. PostgreSQL declaration/value bounds; validate the selected encoding and character/byte counts
   before allocation, bounded cancellation through long transforms/regex work.
7. Multibyte limits/truncation/padding, embedded bytea zero, UUID spellings,
   collation equality/order/hash and text/binary/typmod round trips.
8. Shared provenance; #803 strings, #816 bytea/UUID, #796 collation grammar,
   #904 identity/coercion, #817 truthful catalog metadata.

## Date/time/calendar families — #814/#815

1. date, time/timestamp with/without time zone, timestamptz/timetz and interval
   field/precision forms; PostgreSQL DateStyle/IntervalStyle and named-zone input.
2. Date is signed days from 2000-01-01; local timestamp and timestamptz are distinct
   signed microseconds from that epoch, latter an instant. Time admits 24:00;
   timetz stores time and seconds west of UTC. Interval stores signed i64 micros,
   i32 days, i32 months; never reduce calendar months to fixed seconds. Include
   supported infinities. Comparison/hash/justify/age and date arithmetic use the
   exact PostgreSQL routines, including interval comparison's own equivalences.
3. OIDs 1082/1083/1114/1184/1266/1186 and standard arrays. Binary date i32; time,
   timestamp and timestamptz i64; timetz i64+i32; interval i64 micros+i32 days+i32
   months, network order. Temporal p=0..6 uses actual typmod routines; interval
   packs field range/precision with PostgreSQL INTERVAL_TYPMOD.
4. Tag 1/version 1. Exact PostgreSQL epoch/sentinels, not Unix string storage.
   IANA data from the pinned source is 2026c; freeze tzdata.zi and abbreviation
   hashes. Reference executable/provider artifacts are also hashed, not inferred.
5. TimeZone, DateStyle, IntervalStyle, timezone data and clock lifetimes participate
   in relevant planning/evaluation. Transaction/statement/wall clocks remain
   distinct; old prepared constants preserve PostgreSQL binding-time behavior.
6. PostgreSQL date/time/interval ranges and precision, checked calendar arithmetic,
   no host-time or chrono range restriction as final acceptance. Account/cancel
   parsing/formatting/series expansion with no partial mutation.
7. Epoch/extrema/infinities, 24:00, fixed offset and IANA DST gap/fold, BC/AD,
   interval month/day distinctions, precision/negative values and clock scopes.
8. Shared provenance; #814 values/timezones, #815 clocks/functions, #905 ranges
   consume completed temporal primitive records rather than constraints.

## JSON, JSONB and JSONPATH — #809/#810/#811

1. PostgreSQL JSON/JSONB/jsonpath literals, constructors, casts, SQL/JSON forms.
2. JSON retains source token/whitespace/duplicate-key representation and has no
   invented equality/order/hash. JSONB validates PostgreSQL numeric domain,
   normalization/duplicate-key-last and equality/order/hash/containment laws.
   JSON document null is non-NULL. jsonpath uses PostgreSQL lax/strict and error
   rules. SQL NULL, missing path and JSON null are distinct.
3. OIDs 114/3802/4072, arrays 199/3807/4073, typlen -1, typmod -1. JSON binary is
   its text bytes; JSONB and jsonpath send a version byte 1 then their actual
   canonical PostgreSQL output text bytes. Receive validation is type-specific.
4. Tag 1/version 1, real send payload, not PostgreSQL internal Jsonb containers.
   JSON numbers remain exact tokens/numeric values, never serde f64. Historical
   v2 JSONB aliases retain actual JSON/document interpretation on export.
5. Distinct identities and operator signatures; generated/index/aggregate paths
   depend on exact type and collation where relevant, invalidate on changes.
6. PostgreSQL value/string/numeric limits, accounted nested parse/path work and
   bounded cancellation. Reject unsupported versions/invalid Unicode, no flattening.
7. >53-bit numbers, duplicate keys, whitespace, SQL NULL/document null, path
   error/suppression, no JSON equality, canonical binary round trips and restart.
8. Shared provenance; #809 operators/path, #810 constructors/SQLJSON, #811 aggregates.

## Bit and network families — #816

1. BIT/VARBIT/bit varying binary/hex literals and casts; inet/cidr/macaddr/macaddr8
   accepted PostgreSQL IPv4/IPv6 and hardware-address spellings.
2. Bit length and leading zeros are logical data; fixed/variable typmod, explicit
   cast truncation/padding and bitwise/shift rules use PostgreSQL. Network prefix,
   host bits and address family remain distinct; cidr rejects host bits where
   PostgreSQL does. Comparison/equality/hash are the type's actual operators.
3. OIDs 1560/1562/869/650/829/774 with standard arrays. Bit wire is i32 bit length
   then MSB-first ceil(n/8) bytes; send zeroes unused low padding. inet/cidr wire:
   u8 PostgreSQL family (2 IPv4,3 IPv6), u8 mask bits, u8 cidr flag, u8 byte length,
   then 4/16 address bytes. MAC payloads 6/8 bytes. Exact receive quirks follow source.
4. Tag 1/version 1 with validated send payload. bytea and network/bit are not aliases.
5. Immutable built-ins, distinct arrays and operator/opclass dependencies.
6. Signed bit-length and PostgreSQL declaration/value limits, checked byte products,
   exact address lengths/masks and accounted bulk operations/cancellation.
7. Leading zeros/padding, typmod/cast errors, IPv4/IPv6 masks/hosts, MAC6/MAC8,
   malformed binary framing and scalar/index/array round trips.
8. Shared provenance; #816 values/functions, #904 registry, #912 operators/families.

## Refcursor — #821

1. refcursor literals/casts and cursor-returning routines; PostgreSQL cursor
   declarations/fetch/move/close and transaction lifetimes belong to #821.
2. Portal-name byte identity is separate from the owned portal itself; a stored
   refcursor value does not persist a live session cursor across restart.
3. OID 1790/array 2201, typlen -1, typmod -1; actual text input/output and
   textsend/recv codecs. Do not replace its result descriptor with TEXT.
4. Tag 1/version 1, database-encoding name bytes under actual send rules.
5. Session/transaction portal ownership and security/search_path semantics;
   catalog type identity is stable, live portal state has its proper lifetime.
6. Account name/portal buffers and cursor batches; cancellation/close/rollback
   release portal locks and retained owners with deterministic missing-portal errors.
7. Named/unnamed cursors, parameter/result descriptors, returned refcursor names,
   transaction boundaries, cancellation, arrays and stored-name restart behavior.
8. Shared provenance; #821 sessions/cursors, #904 codecs, routine owners integrate.

## Catalog int2vector/oidvector — #817/#904

1. Distinct PostgreSQL int2vector and oidvector literal/cast types; these are not
   the user VECTOR(n) extension profile or unrestricted standard SQL arrays.
2. Their actual base-type input/receive rules constrain one-dimensional,
   zero-based, non-NULL int2/OID elements. Preserve legal empty-vector value
   identity and any PostgreSQL text-versus-binary admission asymmetry.
3. OIDs 22/30, generated arrays 1006/1013, typlen -1. Their actual send payload
   uses PostgreSQL array framing; distinct receive routines enforce vector laws.
4. Tag 1/version 1, actual specialized built-in send bytes, not generic tag4
   shape normalization. Durable decode preserves legal send-produced empty vectors
   even if the PostgreSQL public receiver rejects that particular empty frame.
5. Built-in descriptor/subscript/catalog dependencies; arrays of these base types
   use their own standard array identities and element codec.
6. Check int2/OID width, counts, zero lower bound/NULL restrictions and accounted
   work. Never classify an arbitrary type as a standard array by category A alone.
7. Empty/nonempty text and binary, zero-based subscripts, element/domain mismatch,
   catalog column descriptors, malformed dimensions/NULLs and durable restart.
8. Shared provenance; #817 catalogs, #904 shared primitive/codec representation.

## OID identifiers, tuple IDs and snapshots — #816/#818

1. oid, all reg* identities, xid/xid8/cid/tid, pg_lsn, pg_snapshot/txid_snapshot,
   PostgreSQL typed literals/casts and catalog name lookup.
2. Unsigned OID/xid/cid 32-bit, xid8/LSN 64-bit, tid block u32/offset u16.
   OID/reg* name visibility and error laws use catalog/search_path/privileges;
   xid wraparound comparison is its own law, not signed integer ordering.
   Snapshot xmin/xmax/xip admission/order and visibility use the reference routines.
   A pg_lsn value does not imply replication support.
3. Registry OIDs/arrays; OID/reg*/xid/cid binary u32, xid8/LSN u64, tid u32+u16.
   Snapshot wire exact `pg_snapshot_send`/`txid_snapshot_send`: u32 xip count,
   u64 xmin, u64 xmax, then u64 xip values. Types/descriptors remain distinct.
4. Tag 1/version 1; preserve logical OID bits and restore catalog identity mapping
   on export. Never rewrite an arbitrary OID datum by guessing it is an object ID.
5. Stable catalog objects, search_path/context and generation-bound lookup caches;
   root publication/restored OIDs prevent rename/restart drift.
6. Checked unsigned bounds, lengths/counts and sorted snapshot IDs; no integer
   cast or transaction-ID constructor is invented when PostgreSQL rejects it.
7. Exact unsigned extrema/LSN/tuple bytes, catalog qualified/unqualified lookup,
   rename/drop/rollback, malformed snapshots and integer→xid cast error controls.
8. Shared provenance; #816 pg_lsn/data, #818 reg* resolution, #819 visibility,
   #817 catalogs, #904 shared identity rather than independent OID helpers.

## Text search — #799

1. tsvector/tsquery literals and constructors, language/config/dictionary lookup.
2. Lexemes, weights, positions and query tree/operators follow PostgreSQL;
   normalization/dictionary version is explicit. Use actual comparison/hash
   signatures; existing BM25 document carriers are not these SQL identities.
3. OIDs 3614/3615, arrays 3643/3645. Exact `tsvectorsend/recv` and
   `tsquerysend/recv` grammars from the pinned source; preserve token counts,
   null-terminated lexeme wire strings, position/weight words and query node tags.
4. Tag 1/version 1, send payload, not PostgreSQL index/internal heap bytes.
5. Dictionary/configuration/routine dependencies, catalog generation and index
   rebuild/invalidation; no C/plugin ABI is implied by SQL configuration support.
6. PostgreSQL lexeme/position/count limits, checked nested query work and bounded
   cancellation through parsing/dictionaries/matching and index construction.
7. Lexeme ordering/weights/positions, phrase/Boolean queries, empty/special lexemes,
   malformed counts/nodes and exact text/binary/index/overlay/restart witnesses.
8. Shared provenance; #799 SQL search, #816 identifiers, #904 codecs, #848 qualification.

## XML — #906

1. XML typed literals, XMLPARSE/XMLSERIALIZE, constructors, XPath/XMLTABLE and
   PostgreSQL document/content casts under xmloption and encoding rules.
2. Preserve valid document/content identity and XML declaration/encoding behavior;
   no invented XML equality/order/hash. Match namespace/XPath/node-set semantics
   and errors with a pinned compatible XML library, provider version in receipts.
3. OID 142/array 143; XML binary is `xml_send`'s actual external text payload,
   validated by `xml_recv`; actual bytes and XML declaration follow client
   encoding. Durable serialization fixes client encoding to database encoding
   and follows the actual XML send/receive declaration rules.
4. Tag 1/version 1, send payload, no DOM/Rust/XML-library serialization ABI.
5. Function/settings/library dependencies and cache invalidation; no semantic
   normalization merely for index/storage convenience.
6. Account/cancel parsing, node sets and serialization; never expand external
   entities or introduce filesystem/network behavior beyond PostgreSQL's selected
   XML parser policy. Invalid encodings/documents error before mutation.
7. Document/content, namespaces, XML declaration/encoding, malformed input,
   missing equality, SQL NULL and text/binary/array/restart witnesses.
8. Shared provenance; #906 complete XML behavior, #904 registry, #848 library-pinned oracle.

## Geometry — #907

1. point/lseg/line/box/path/polygon/circle literals/constructors/casts, PostgreSQL
   native geometry operators/functions (not an inferred PostGIS extension).
2. Exact PostgreSQL floating-coordinate, normalization, epsilon/degenerate cases
   and operator-specific comparisons. Shape equality does not imply a total
   order/hash; expose only actual supported signatures/opclasses.
3. OIDs 600/601/628/603/602/604/718 with registry arrays. Point two float8; lseg
   and box four; line three; circle three. Path u8 closed flag+i32 point count+
   point coordinates; polygon i32 count+points. Network-order float8 throughout.
4. Tag 1/version 1, actual normalized send payload, never internal bbox/cache bytes.
5. Distinct shapes and correct operator/opclass dependencies; copied descriptors
   cannot silently substitute generic vector distance behavior.
6. Checked point counts/products and coordinate inputs, accounted geometric work,
   cancellation for large paths/polygons. Floating special rules follow source.
7. Every shape, normalized box corners, open/closed paths, degenerate/intersection
   cases, malformed counts and text/binary/index/array/restart witnesses.
8. Shared provenance; #907 values/operators, #912 SQL operator families, #904 codecs.

## Money — #908

1. money literals/casts and supported arithmetic/aggregate functions.
2. Signed i64 amount in locale-dependent fractional units; locale input/output,
   rounding, overflow, comparisons and casts match PostgreSQL. Preserve units and
   the pinned lc_monetary provider; never treat all money as fixed decimal(?,2).
3. OID 790/array 791, typlen 8, typmod -1; binary signed i64 network order.
4. Tag 1/version 1, actual signed amount; catalog/export records preserve the
   database locale/provider settings needed to interpret/format it.
5. Locale settings/provider version and routine/type dependencies in contexts.
6. Checked i64 arithmetic/conversions, accounted parsing/formatting/bulk work.
7. C and selected non-C locales with exact provider pin, fractions/rounding,
   overflow/mixed arithmetic and text/binary/array/restart/export witnesses.
8. Shared provenance; #908 money, #821 settings, #904 codecs; no locale-independent promise.

## ACL items — #817/#831/#904

1. aclitem text input/output, catalog ACL arrays and PostgreSQL privilege syntax.
2. Stable grantor/grantee role identities, PUBLIC grantee 0, valid PostgreSQL
   privilege/grant-option masks and object-kind privilege validation.
3. OID 1033/array 1034, catalog typlen 16. No PostgreSQL binary send/receive;
   binary wire output errors 42883, including containers when element codec needed.
4. Approved private tag 9/version 1: u64 grantor, u64 grantee, u32 granted mask,
   u32 grant-option mask; grant-option subset of granted, network byte order.
   This is deliberately not a public binary ABI. SQL NULL has no ACL payload.
5. Role rename preserves references; role drop/reassign/grants/dependencies and
   restart/export are transactionally coherent and invalidate authorization caches.
6. Checked masks/identities and accounted large ACL arrays, cancellation/rollback
   before publication. Never create a missing role to decode an ACL item.
7. Text oracle, binary 42883, PUBLIC/role rename/drop, grant options, malformed
   masks/IDs, privilege enforcement and durable/export/restart witnesses.
8. Shared provenance and explicit common-v3 ACL approval; #831 security, #817 catalogs.

## Standard arrays — #807/#808

1. PostgreSQL array constructors/literals/subscripts/slices/concatenation, any
   registered admissible element type, explicit dimensions/lower bounds.
2. Zero-dimensional empty and 1–6 dimensional rectangular shapes, signed lower
   bounds, row-major order; SQL NULL array/element and JSON null differ. Equality,
   order/hash use element laws and shape metadata, never flattening/JSON equality.
3. Standard generated array OIDs in registry, element OID from descriptor. Wire
   i32 ndim+i32 flags+u32 element OID, then i32 length/lower-bound pairs, then
   i32 element lengths (-1 SQL NULL) and exact element wire payloads. PostgreSQL
   receive's built-in/named element-OID mismatch behavior is normative.
4. Tag 4/version 1: u32 ndim, i32 length/lower pairs, recursively framed nullable
   elements. Element identity/revision comes from the immutable array descriptor,
   not wire OID bytes or nesting a new array identity per dimension.
5. Element/domain/collation dependencies and retained revisions; arrays of named
   values have stable separate type IDs/OIDs and participate in cascade/rollback.
6. Checked dimension products/upper bounds and element/bitmap sizes before reserve;
   PostgreSQL MAXDIM=6 and value bounds. Domain NULL elements still enforce the
   domain's constraints. Account/cancel per-element work with no partial write.
7. Empty/multidimensional/non-one lower bounds, NULL/JSON null, every element family,
   domain NULL checks, shape/type/count errors, codecs, indexes, restart and upgrade.
8. Shared provenance; #807 representation/functions, #808 expansion/ordinality,
   #904 dispatch. Earlier one-dimensional acceptance is intermediate only.

## Named enums — #827

1. CREATE/ALTER/DROP TYPE enum and type-qualified literals/casts.
2. Declared label order, immutable member identity through rename and insertion;
   equality/order/hash and enum polymorphism follow PostgreSQL. Label length and
   unsafe-use-of-uncommitted-new-enum-value rules remain normative.
3. Stable named scalar/array OIDs and typlen 4 metadata, text label wire payload
   for enum_send/recv. Resolve labels against the proper database/type revision.
4. Tag 2/version 1, immutable u64 member ID, not label text or ordinal/float order.
5. Publish immutable ordered member descriptors with transaction/savepoint rules,
   retain old members while referenced; rename does not rewrite value identity.
6. Checked monotonic identities and label/count bounds, no committed ID reuse;
   cancellation/rollback cannot expose partial label order or allocator wrap.
7. Add before/after, rename, equality/order/hash, arrays, cross-session visibility,
   uncommitted additions, restart/rollback and stable export/import identities.
8. Shared provenance; #827 lifecycle, #904 registry, #833 catalog publication.

## Composite and transient record — #828

1. Named composite/table row types, ROW syntax/casts/field access and record
   routine outputs. Anonymous record is a transient identity, not a stored column.
2. Field order/types/collations, SQL NULL record versus non-NULL all-NULL fields,
   PostgreSQL row IS NULL versus scalar NULL identity, equality/order/hash laws.
3. Named OIDs and standard array identity; transient record OID 2249 and actual
   contextual typmod. Wire i32 field count, then u32 field type OID+i32 payload
   length (-1 NULL)+field wire bytes. Descriptor, not caller bytes, owns field types.
4. Tag 3/version 1, u32 positional count and recursively framed nullable fields;
   type/revision inherited from the immutable enclosing descriptor. Anonymous
   records need a named/retained declared descriptor before durable column admission.
5. Table/composite dependencies, dropped attributes and versioned schema evolution;
   retain positional old descriptors and invalidate prepared/cache field bindings.
6. Check counts/nesting/lengths/constraints before reserve; use accounted explicit
   traversal stacks and bounded cancellation, no unbounded native recursion.
7. Named/anonymous descriptors, all-NULL versus SQL NULL, dropped/renamed fields,
   domain fields, malformed wire counts/OIDs and text/binary/restart/export witnesses.
8. Shared provenance; #828 after #826 table foundation; range primitives belong to
   #905 and do not wait on this owner.

## Domains — #828

1. CREATE/ALTER/DROP DOMAIN, base typmods/collation, casts and assignment contexts.
2. Distinct declared identity with base value semantics and domain CHECK/NOT NULL
   validation, including SQL NULL. PostgreSQL domain flattening in common-type and
   expression/result resolution is explicit, not accidental loss of catalog identity.
3. Stable named/array OIDs, actual domain pg_type metadata; use base wire codecs
   and PostgreSQL's base-type RowDescription where domain output is flattened.
4. Tag 7/version 1, recursively framed non-NULL base datum. SQL NULL uses the outer
   marker only; reject non-NULL domain payload wrapping a NULL base datum.
5. Immutable base/constraint descriptors, dependency publication/revalidation,
   named identity preserved on rename; old rows retain descriptor meaning.
6. Bound/check constraint evaluation with shared controls; SQL NULL domain/array
   elements cannot bypass NOT NULL/CHECK. No partial mutation on failure.
7. Domain text/binary/base output OID, NOT NULL/CHECK including NULL, arrays,
   nested casts, rollback/savepoints, constraint changes and restart/export witnesses.
8. Shared provenance; #828 after #826, #904 coercion/descriptors, #833 publication.

## Ranges and multiranges — #905

1. Built-in int4/int8/numeric/date/timestamp/timestamptz ranges and multiranges;
   custom CREATE TYPE range metadata/constructors when its routines are complete.
2. Empty/unbounded/inclusive/exclusive distinctions, subtype order/collation,
   discrete canonicalization and continuous subtype laws. Normalize multiranges
   as ordered disjoint nonadjacent canonical ranges using subtype semantics.
3. Registry built-in OIDs/arrays and stable custom identities. Range wire u8 flags
   then i32 length+subtype bytes for each present bound. Multirange i32 count then
   i32 length+range wire bytes. Flag constants/canonicalization use pinned source.
4. Tag 5/version 1: u8 canonical flags then framed non-NULL finite subtype bounds.
   Tag 6/version 1: u32 count then u32 length+canonical durable range payloads.
   Empty flags 0x01, inclusion 0x02/0x04, unbounded 0x08/0x10; reject unsupported
   internal NULL/GiST flags and noncanonical durable combinations.
5. Range/subtype/opclass/collation/canonical/subdiff/multirange dependencies;
   custom routine/index integration is staged after primitive codecs.
6. Checked lengths/counts/canonicalization and bounded cancellation; no expanding
   an unbounded range into elements. Bound NULL is not an unbounded flag.
7. All six built-in families, custom bounds, empty/infinite, discrete normalization,
   multirange merge/order, malformed flags/lengths and scalar/index/restart witnesses.
8. Shared provenance; #905 primitives consume numeric/temporal completed slices,
   then #824 constraints → #826 tables → #828 composites/domains. No reverse cycle.

## Pseudo-types and internal SQL-visible carriers — #817/#818/#904

1. All immutable registry identities, including polymorphic any*/anycompatible*,
   record/void/unknown/cstring, handler/internal/trigger/event-trigger and catalog
   internal carriers. PostgreSQL legal contexts/input errors are normative.
2. Enforce actual polymorphic argument/result consistency and compatible common
   type resolution. A dummy I/O routine or catalog row does not admit a stored
   user value. No invented equality/order/hash. Internal catalog values may be
   produced only by their owning SQL/catalog functionality under its own records.
3. Exact built-in metadata and actual I/O/error routines. gtsvector has no usable
   public input/binary ABI; pg_node_tree input/receive errors 0A000 though its
   catalog output exists. Handler pseudo-types retain the architectural ABI exceptions.
4. No fabricated codec for illegal stored non-NULL values. Legal SQL NULL columns
   use the row's NULL bitmap. Actual SQL-produced internal values need their
   owner's exact send/semantic record before durable non-NULL admission.
5. Runtime callable/record descriptors are scoped to session/catalog generation;
   restart rebuilds them from authoritative routine/catalog definitions.
6. Checked descriptor consistency, contextual errors and cancellation; no raw
   pointer, PostgreSQL internal memory image or Rust type address becomes a value.
7. Polymorphic families, unknown/all-NULL resolution, context/DDL/constructor
   rejection, catalog output versus public input and descriptor/error witnesses.
8. Shared provenance; #904 resolver, #817 catalogs, #818 identities; #914 owns
   statistics carriers and #842 index-internal carriers before their runtime.

## Selected existing vector profile — #844

1. Retain the currently supported finite f32 VECTOR(n) profile as an intermediate
   compatibility boundary. Bare vector/pgvector ABI/version selection belongs to
   #844 and must be exact before that new public behavior is implemented.
2. Finite components, exact positive dimension, SQL NULL distinct from any vector;
   existing metric laws and error ownership remain authoritative.
3. Existing `100000+n` scalar wire identities and legacy vector-array identities
   remain reserved during transition. They are not standard pgvector OIDs or a
   selected pgvector version promise. The #844 extended wire/extension matrix stays open.
4. Approved tag 8/version 1, u32 count and exact network f32 components. Use a
   retained descriptor profile/version and dimensions, not a guessed extension ABI.
5. Rebuild derived vector/ANN sidecars from authoritative imported vectors;
   preserve catalog/profile/generation dependencies and existing-data interpretation.
6. Current dimensional wire bound 1..32767 and finite validation remain existing
   support only; checked products/accounting and cancellation apply everywhere.
7. Existing exact/ANN/cross-path and restart witnesses are retained. New #844
   bare casts/extension/operator-class/wire identities require separate pinned
   extension oracle and full qualification, never inferred from PostgreSQL core.
8. Common durable vector direction approved; #844 extended public ABI is an
   explicit staged record gate, not an invented exclusion of selected capabilities.

## Reference evidence interpretation

The accompanying 31-case observations contain 93 reference paths, including
expected errors; they are not 93 Cassie passes. Eleven focused encoding/float
observations and four built-in composite/six range descriptors refine the
records. Private codec vectors specify future acceptance, not an implemented
encoder/decoder. PostgreSQL fixture named OIDs belong to that isolated reference
catalog. Compare their declared identity/dependency graph through the approved
Cassie mapping; retain exact built-in OIDs and qualify stable named wire identities
independently. Do not blindly require reference-instance named OIDs, or normalize
away a wrong/missing Cassie descriptor. Every Cassie runtime cell remains pending.
