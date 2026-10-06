# Finite SQL type and wire contract

This is the selected current-type contract for [#752](https://github.com/cntryl/cassie/issues/752), under [Query Engine Target 1](query-engine-target.md). It preserves the existing logical types and Midge row/field encodings. The selection is recorded in [the type-contract decision](https://github.com/cntryl/cassie/issues/752#issuecomment-5994420099). Qualification is in progress; a selected outcome or source mapping is not an executed acceptance result.

## Metadata and input boundaries

| Topic | Contract |
| --- | --- |
| Unknown carrier | Result and catalog descriptors use OID 705, typlen −2 and typmod −1. Cassie's logical `null` name is a metadata alias. SQL NULL Bind/DataRow field length remains −1. The descriptor convention does not introduce C-string storage. |
| Numeric text adapters | OID 700 decodes to f64. OID 1700 preserves an exactly parseable i64; other valid numeric text decodes to f64. These are input adapters, without binary codecs or new logical FLOAT4/NUMERIC types. |
| Numeric result metadata | An existing-type CAST is required for metadata-ambiguous unconstrained output. Direct `SELECT $1` under 700/1700 rejects 0A000 before RowDescription/DataRow for NULL and nonNULL. Contextual DML, predicates, independently fixed result types and explicit Boolean casts remain supported. ParameterDescription retains the declared OID. Numeric NULL retains adapter provenance at Boolean sinks. |
| Vector arrays | SQL/storage declarations and the durable array OID formula remain unchanged. ARRAY(VECTOR(n)) pgwire identity/codecs are excluded and reject 0A000 before statement/portal RowDescription or DataRow, including text, empty and NULL-only output. BOOL[] 34016 and scalar VECTOR remain supported. |
| Finite type discovery | `pg_catalog.pg_type` has one row per supported wire OID: existing scalar families, their 14 nonNull/nonVector scalar-array families, and supported scalar vector dimensions referenced directly by current-database collection/view schema fields. A helper without a session database retains its existing unfiltered scope. ARRAY(VECTOR) contributes neither an array registry row nor nested scalar dimension discovery. CHAR/VARCHAR family rows are unparameterized; column typmod retains length. Wider PostgreSQL catalog/extension discovery is excluded. |
| Stored output identifiers | Finite output qualification preserves stored fields' delimited final spelling and output presentation names. Own source qualifiers are matched as parsed components; dots inside quoted fields do not create qualifiers. The selected stored-field and derived-alias cases compare descriptors and values in both formats. This private prerequisite retains existing identifier semantics; the wider relational matrix remains owned by #761. |
| Row identity in output | A base table always exposes physical `_id` for explicit lookup. Only a base table without declared `id` supplies its legacy `id` alias. Derived/CTE explicit `_id` exports do not invent `id` aliases that shadow another source's declared field. SELECT wildcard retains its existing `id` presentation and full row width through derived sources; DML RETURNING wildcard retains its existing `_id` presentation in Describe and Execute. These corrections change no stored identity or schema format. |

## Current logical types

The table covers all 17 current DataType variants, including parameterized VECTOR and ARRAY. OIDs are Cassie's finite supported identities. A selected descriptor is a contract; fresh qualification remains recorded separately. Character registry names omit column modifiers, which remain in declared schemas.

| Logical type | Name | Result OID | typlen | typmod |
| --- | --- | --- | --- | --- |
| Null | null | 705 | -2 | -1 |
| SmallInt | smallint | 21 | 2 | -1 |
| Int | int | 23 | 4 | -1 |
| BigInt | bigint | 20 | 8 | -1 |
| Float | float | 701 | 8 | -1 |
| Boolean | boolean | 16 | 1 | -1 |
| Text | text | 25 | -1 | -1 |
| Char | char(n); internal None is char, parsed bare CHAR is char(1) | 1042 | -1 | n+4; bare/default n=1 |
| Varchar | varchar(n); no length is varchar | 1043 | -1 | n+4 when representable, otherwise -1; bare unlimited |
| Uuid | uuid | 2950 | 16 | -1 |
| Bytea | bytea | 17 | -1 | -1 |
| Date | date | 1082 | 4 | -1 |
| Time | time | 1083 | 8 | -1 |
| Timestamp | timestamp | 1114 | 8 | -1 |
| Vector(n) | vector(n) | 100000+n | -1 | -1 |
| Json | json | 114 | -1 | -1 |
| Array(T) | T.type_name() followed by [] | 34000 + element_OID % 10000; vector element uses 33000+n residue | -1 | -1 |

For ARRAY, the listed formula is the existing durable identity calculation. Pgwire accepts only the following 14 scalar element families; VECTOR arrays remain excluded even where their residue collides with a supported scalar-array OID.

| Element family | Element OID | Array OID |
| --- | --- | --- |
| Boolean | 16 | 34016 |
| SmallInt | 21 | 34021 |
| Int | 23 | 34023 |
| BigInt | 20 | 34020 |
| Float | 701 | 34701 |
| Text | 25 | 34025 |
| Char | 1042 | 35042 |
| Varchar | 1043 | 35043 |
| Uuid | 2950 | 36950 |
| Bytea | 17 | 34017 |
| Date | 1082 | 35082 |
| Time | 1083 | 35083 |
| Timestamp | 1114 | 35114 |
| Json | 114 | 34114 |

ARRAY typmod remains −1, including ARRAY(CHAR(n)) and ARRAY(VARCHAR(n)). The complete inner logical DataType retains n for validation. This introduces no new array modifier ABI.

Both brace and JSON-bracket text array inputs validate the declared scalar
element family before Bind completes or COPY stages the row. Integer elements
must fit their declared i16/i32/i64 domain; JSON-bracket numeric/Boolean/string
families do not accept values from another family. Declared FLOAT input selects
finite f64 values, including the same decimal rounding as scalar text FLOAT;
BIGINT input retains exact i64 integers. UUID, BYTEA and temporal elements use
their existing scalar canonicalizers. SQL NULL slots and the selected JSON
document/origin rules remain unchanged. A wire array OID carries no character
length: CHAR with an unknown modifier retains raw input until a target supplies
its bound; known CHAR(n)/VARCHAR(n) bounds still apply. Missing VARCHAR length
means unlimited scalar and array storage, while explicit bounds and bare
CHAR(1) remain enforced. These corrections preserve stored type tags and the
encoding layout.

Declared numeric JSON-bracket array inputs retain original numeric tokens:
FLOAT uses the scalar std f64 parser, including finite endpoints, subnormals
and signed zero; integer families use exact i64 parsing before declared-width
validation. Lexical integer `-0` normalizes to zero, while fractional and
exponent-shaped integer inputs remain excluded. Complete JSON grammar is
validated before element normalization. Document JSON parsing is unchanged.

## Value and storage representations

These existing carriers and encodings remain the recovery authority; physical typed buffers introduced later must preserve their logical values and declared restrictions.

| Logical type | SQL spellings | Value carrier | Existing storage |
| --- | --- | --- | --- |
| Null | null | Value::Null | TYPE_NULL / absent JSON field |
| SmallInt | smallint, int2 | Value::Int64 constrained to i16 | TYPE_I64, 8-byte network-endian payload |
| Int | int, integer, int4 | Value::Int64 constrained to i32 | TYPE_I64, 8-byte payload |
| BigInt | bigint, int8 | Value::Int64; preserve beyond 2^53 | TYPE_I64, 8-byte payload |
| Float | float, double, numeric, decimal | Value::Float64; IEEE f64, not exact decimal | TYPE_F64, 8-byte payload; writes reject non-finite JSON-backed values |
| Boolean | boolean, bool | Value::Bool | TYPE_BOOL, one byte |
| Text | text, string | Value::String UTF-8 | TYPE_STRING UTF-8 |
| Char | char, char(n) | Value::String, character count bounded; trailing ASCII blanks removed | TYPE_STRING canonical unpadded UTF-8 |
| Varchar | varchar, varchar(n) | Value::String, optional character bound | TYPE_STRING UTF-8 |
| Uuid | uuid | Value::String canonical lower-case UUID | TYPE_UUID, exact 16 bytes |
| Bytea | bytea | Value::String canonical lower-case hex with \x prefix | TYPE_BYTEA raw bytes |
| Date | date | Value::String canonical YYYY-MM-DD | TYPE_DATE canonical UTF-8 |
| Time | time, time(p), 0 <= p <= 6 | Value::String canonical HH:MM:SS[.ffffff] | TYPE_TIME canonical UTF-8 |
| Timestamp | timestamp, timestamp(p), 0 <= p <= 6 | Value::String canonical UTC instant YYYY-MM-DDTHH:MM:SS.ffffffZ | TYPE_TIMESTAMP canonical UTC UTF-8 |
| Vector(n) | vector(n), positive n | Value::Vector(Vec<f32>), n fixed, finite components | TYPE_VECTOR_F32, u32 dimension prefix then exact finite f32 elements |
| Json | json, jsonb | Value::Json for document-only null/string/container/unsigned-overflow values; existing primitive carriers for signed numbers, f64 and booleans; keep SQL Null distinct | TYPE_JSON canonical JSON bytes; absent key/TYPE_NULL is SQL NULL |
| Array(T) | T[]; parser rejects array-of-array declarations | Value::Json(Array), logical element type retained separately | TYPE_ARRAY count then per-element tag/payload; same row encoding, no new layout |

## Scalar and array codecs

Text and binary support below describe the selected finite current profile. Numeric input adapters do not imply matching result or binary ABIs. SQL NULL framing is independent of the nonNULL codec. JSON array elements use the JSON document codec; known document-null scalar elements have the explicit exclusion described below.

| Logical type | Text codec | Binary codec |
| --- | --- | --- |
| Null | NULL framing; binary unknown codec is UTF-8 only for non-NULL payloads | Null field -1; unknown 705 registry maps Text |
| SmallInt | trim + exact i64 parse + i16 range; decimal output | exact 2-byte network-endian signed integer |
| Int | trim + exact i64 parse + i32 range; decimal output | exact 4-byte network-endian signed integer |
| BigInt | trim + exact i64 parse; decimal output | exact 8-byte network-endian signed integer; float-backed output requires finite integral in-range value |
| Float | OID 700/701 -> f64; OID 1700 -> i64 if integral/in-range otherwise f64; text output Infinity/-Infinity/NaN where query values exist | OID 701: exact 8-byte network-endian f64; inexact i64-to-f64 output rejected; OID 700 and 1700 unsupported |
| Boolean | PostgreSQL-style accepted vocabulary through shared boolean parser; result t/f | exact one-byte 0 or 1 only |
| Text | raw UTF-8 | raw UTF-8, invalid UTF-8 rejected |
| Char | raw input -> write validation; raw unpadded output | UTF-8 text payload |
| Varchar | raw input -> write validation; raw output | UTF-8 text payload |
| Uuid | parse and canonicalize UUID spelling | exact 16 UUID bytes |
| Bytea | hex \x... only, even hex digits; canonical text | raw bytes |
| Date | shared validated date parser; canonical output | exact 4 signed bytes: days from 2000-01-01 |
| Time | shared validated clock parser, max six fractional digits | exact 8 signed bytes: microseconds from midnight; 0 <= value < 86,400,000,000 |
| Timestamp | shared parser normalizes explicit offsets and treats offset-free input as UTC | exact 8 signed bytes: microseconds from 2000-01-01; Euclidean division for negative timestamps |
| Vector(n) | text input remains a string until selected expression/write normalization; output [x,y,...] | private OID family n=1..32767; int16 dimensions + zero reserved + exact n network-endian f32 values |
| Json | OID 114 parse JSON; output JSON text preserves quoted strings and literal null | OID 114 JSON text bytes, no JSONB version prefix |
| Array(T) | private known scalar-array OIDs parse brace literals or JSON arrays; output JSON brackets | private known scalar-array OIDs: ndim 0/1; has_null 0/1; exact element OID; lower bound 1; bounded lengths and complete payload; element codec from scalar registry |

Vector text output retains its existing carrier spellings: native stored reads use f32 display, while JSON-backed RETURNING uses JSON number spelling. For example, `[1.5,-2]` and `[1.5,-2.0]` have the same selected vector meaning. The independent binary golden is identical across those paths; no canonical text-format migration is selected.

Registered scalar VECTOR binary output accepts native vectors, JSON-array RETURNING values and valid declared vector text parameters. Text encoding validates the exact declared component count and finite f32 conversion before allocating output; it does not reparse arbitrary scalar strings or alter other scalar codecs. Nullable vector writes retain SQL NULL under the existing row encoding. A nonnullable storage field and SQL NOT NULL constraints continue to reject NULL.

Selected declared-vector text output and write normalization borrow validated
JSON numeric tokens and parse them with the standard f64 parser before the
existing finite-f32 range check. This preserves the exact f32 endpoint spellings
that the default JSON number parser can round outside that range. A shared
private visitor validates count, grammar and component range before allocating
the output buffer; replay fills only the validated declared width. The default
JSON parser, vector range, wire OIDs and durable vector representation remain
unchanged. Invalid write shapes retain their existing schema-validation path;
out-of-range components retain their existing range error.

## Comparison and semantic keys

Value-expression forwarding preserves a parameter or column's existing typed
Value carrier through supported COALESCE and function arguments, including
vectors, JSON documents and scalar arrays. Local function arguments retain
their existing precedence over row fields using the same identifier lookup.
Scalar arithmetic, predicate typing and casts retain their existing scalar
evaluation policy. This corrects a lossy bridge; it introduces no typed batch
representation or new stored value.
Value-preserving window arguments retain the same rich carriers, including
ARRAY values, rather than returning their former incidental string spelling.

Directly forwarded JSON document values retain their JSON identity when a
numeric aggregate rejects them. A mixed JSON column containing a string document
therefore reports `function sum(json) does not exist` or
`function avg(json) does not exist` (SQLSTATE 42883), using the existing aggregate
error classifier. This changes the former incidental `text` qualifier, not
numeric aggregate acceptance. Ordinary numeric carriers remain supported;
TEXT and BOOLEAN retain their existing rejection diagnostics. Cast expressions
continue through scalar evaluation and do not promise the direct-forwarding
JSON qualifier.

Direct vector MIN/MAX inputs now retain their native result carrier, matching
the declared VECTOR descriptor. They use the existing whole-vector f32-bit
semantic key instead of incidental string ordering from the former lossy
bridge. Column summaries continue to require row fallback for vector extrema;
this correction does not enable summary acceleration or change distance metrics.

SMALLINT/INT/BIGINT comparisons retain exact i64 values; exact integral f64 values share numeric semantic keys, and NaN keys canonicalize under the current engine policy. Integer-to-float output rejects inexact conversion. BOOLEAN predicate/type semantics reuse merged #432 qualification; NULL retains SQL three-valued predicate rules and null-key join exclusion, with the existing grouping/distinct null key.

String-backed types retain canonical UTF-8 value keys and existing ordering. Declared UUID, BYTEA and temporal canonicalization establishes their stored spelling. Timestamp normalization/order is by the canonical UTC instant; the existing timestamp-shaped TEXT value rule is retained. No PostgreSQL collation parity is claimed. CHAR removes trailing ASCII blanks. ARRAY uses declared elementwise semantic keys/order, compares the first unequal element and then equal-prefix length, and places NULL elements last. Generic JSON keys do not replace the declared array element type.

JSON document predicates compare documents/numeric values recursively. Current primitive signed/f64/Boolean scalar carriers retain their numeric/Boolean keys; document carriers use serialized JSON text keys. Unsigned JSON document numbers beyond i64 preserve their exact document carrier. These policies do not imply universal JSON predicate/hash equivalence. Later typed kernels must retain the selected logical behavior or record a separate scoped correction. Whole-vector value keys use f32 component bits; cosine/dot/L2 retrieval metrics are a separate contract.

## JSON and array element meaning

Top-level SQL NULL and JSON document null remain distinct. SQL NULL uses field length −1. An OID 114 JSON document null uses four document bytes, `null`. JSON string documents retain their quotes; numbers, booleans, objects and nested documents retain JSON meaning in text and binary JSON codecs.

Schema-aware scan, RETURNING and conflict-update conversion preserve unsigned
JSON numbers beyond i64 as document values. This avoids loss through f64 while
retaining ordinary signed/f64/Boolean scalar carriers and their existing keys.
Preserving all JSON scalars as document carriers would change those comparison
and grouping paths; changing only the output codec cannot restore bits lost
earlier. The selected narrow correction uses existing JSON storage and introduces
no unsigned SQL arithmetic type or recovery format.

The existing SQL ARRAY(JSON) container represents direct null elements as SQL NULL. Distinct JSON document-null scalars inside that SQL array are excluded: known document-null input must reject explicitly before it collapses into SQL NULL. This selects no new element tag, validity bitmap or storage encoding. A nonNULL JSON document containing null, such as one array element whose document is `[null]`, remains supported.

Supported binary scalar arrays retain their private OIDs, zero-dimensional empty framing or one dimension with lower bound 1, declared scalar element OID, NULL flags and checked element lengths/counts. A nested JSON document inside one ARRAY(JSON) element is not a multidimensional SQL array. Standard PostgreSQL array OIDs and arbitrary dimensions/lower bounds are excluded.

## Exactness and future typed columns

Declared integer widths remain range constrained while the current Value carrier uses i64; values beyond 2^53 must never pass through a lossy f64 conversion. FLOAT remains f64. VECTOR components remain finite f32 with their declared dimension. Temporal binary codecs use PostgreSQL's 2000-01-01 epoch, days for DATE and microseconds for TIME/TIMESTAMP; TIMESTAMP retains Cassie's documented UTC normalization. Character length, UUID/BYTEA canonicalization, JSON document meaning and declared array element identity remain part of each logical type.

The dependent [#753](https://github.com/cntryl/cassie/issues/753) contract must preserve these logical identities, exactness, units and SQL validity when choosing private typed-column representations. This document does not introduce the common batch API or claim that future typed kernels exist.

| Current logical type | Future column requirement |
| --- | --- |
| Null | Carry SQL validity separately from a nonNULL value. Logical Null is the existing metadata carrier, not a new declared SQL type or a JSON document-null representation. |
| SmallInt | Signed integer family with exact i16 domain. An i64 buffer is permitted only while declared range checks remain authoritative; never pass through f64. |
| Int | Signed integer family with exact i32 domain. An i64 buffer must retain the declared i32 range; never pass through f64. |
| BigInt | Exact signed i64 values, including both endpoints and integers beyond 2^53. No universal f64 physical carrier. |
| Float | IEEE f64 value family with the existing equality/key/codec policy. Preserve permitted query non-finite values and existing storage-write rejection; do not introduce exact decimal or f32 rounding. |
| Boolean | Boolean/validated one-byte value family. Keep SQL three-valued logic in validity, and reject nonBoolean typed inputs under the merged Boolean contract. |
| Text | UTF-8 string or UTF-8 offset/view family retaining exact text, including a literal `NULL` string. |
| Char | UTF-8 family retaining declared n and current character-count validation and unpadded canonical value. Do not replace n with byte length. |
| Varchar | UTF-8 family retaining optional declared n and current character-count validation. |
| Uuid | Exact 16-byte UUID or canonical UTF-8 fallback, with lossless conversion to the same canonical UUID value. |
| Bytea | Exact byte view or canonical hex UTF-8 fallback, with lossless conversion to the existing raw-byte wire/storage value. |
| Date | Exact calendar-date family or existing canonical UTF-8 fallback. An epoch representation must use whole days relative to 2000-01-01 and preserve the current supported date domain. |
| Time | Exact microsecond-of-day family or canonical UTF-8 fallback; integer units, 0 <= value < 86,400,000,000. |
| Timestamp | Exact microseconds relative to 2000-01-01 or canonical UTC UTF-8 fallback; preserve instant equality/order and offset normalization, including negative epoch values. |
| Vector(n) | Fixed-dimension finite f32 family or exact existing vector fallback. Preserve declared n and f32 components; do not introduce an additional storage dimension cap or ARRAY(VECTOR) wire identity. |
| Json | Rich document family or lossless existing typed fallback retaining document numbers, booleans, quoted strings, objects, arrays and JSON document null, distinct from SQL NULL. Preserve the documented current predicate/group/key policies rather than silently replacing them with one universal numeric or serialized key rule. |
| Array(T) | Rich typed-array family or existing typed fallback retaining the complete inner DataType, order and SQLNULL slots. Preserve CHAR/VARCHAR inner n even though array wire typmod is −1. Retain the selected direct JSON document-null element and VECTOR-array wire exclusions. |

Each family must support a bounded, explicitly owned fallback when a first kernel cannot consume it. A view's lifetime, selection/row identity and memory reservation must preserve the value and validity laws; their concrete indices/offsets/buffers remain #753 decisions. No type may be dropped or coerced merely to fit the first primitive kernel.

## Persistent and recovery contract

SELECT and DML exports use the compiled finite output authority. DML without
RETURNING has zero result columns: Describe emits NoData and successful
execution emits CommandComplete. Other command families retain their existing
execution-result metadata owner; this contract does not replace their command
Describe/capability surface with a new administrative catalog profile.

Pgwire validates exported result types from the already compiled plan before a result-cache hit or any execution side effect. Describe and Execute use the existing metadata owner when there are no parameters and the private parameter-aware output contract when the prepared OID list is nonempty. The latter includes ordinary supported parameter types, because their exported ARRAY and CTE types also require propagation. Both paths apply the same logical VECTOR-array exclusion and execution controls. This preserves ordinary metadata and avoids repeating parameter analysis for queries without parameters; paired source-shape probes qualify the additional parameter path. Fresh, cached and fixed-schema suspended type cases retain their descriptors. Numeric input provenance follows exported positions through CTEs, derived tables and value-contributing set branches. An outer existing-type CAST consumes that provenance. Predicates, constrained assignments and independently fixed result expressions do not require a CAST merely because they use a numeric input adapter.

Transaction-scoped portal lifetime and schema/snapshot changes remain owned by
[#755](https://github.com/cntryl/cassie/issues/755)/[#757](https://github.com/cntryl/cassie/issues/757).
A diagnostic that resumes a named wildcard cursor after Sync and column addition
reproduces descriptor drift in the current protocol path. The approved lifecycle
ends that portal at Sync; its dependent acceptance must reject the resumed name
with 26000 and qualify cleanup. This type slice does not claim that lifecycle
fix or broader catalog-mutation snapshot correctness.

This private validation runs under execution's existing admission, cancellation and deadline controls. It preserves statement authorization and failed-transaction checks before output validation. REST and embedded execution retain their existing interface profiles. It adds no public portal field or durable representation.

No persistent format, layout migration or new stored type is selected here. Stored rows and field values retain their existing Midge encodings and recovery authority. The metadata/codec corrections must preserve existing durable values. Unsupported wire identities or element classes fail before misleading descriptors, result rows or writes; they do not publish a replacement storage representation.

Exact decimal storage, FLOAT4/TIMESTAMPTZ/JSONB result ABIs, pgvector extension ABI, standard array OIDs, a new vector storage dimension cap and a persistent array document-null representation remain excluded. Every selected acceptance case must pass before #752 closes; the finite per-type table and acceptance record will distinguish fresh probes from reused qualification and explicit exclusions.

## Finite acceptance ownership

These test owners qualify the selected contract. Focused red/green evidence is
retained separately from the required full gate and exact published revision.
The active #752 PR record must include that final qualification, review and
merge/closure readback. Named existing tests are reused owners, not evidence
that later typed kernels or every operator/type combination have shipped.

| Owned invariant | Acceptance owners and explicit boundary |
| --- | --- |
| TYPE-001 | `type_catalog_discovery`, `type_unknown_metadata`, `type_scalar_matrix`, `type_scalar_array_matrix`, `type_declared_null_matrix`, `type_output_identifier_identity` and `type_output_source_seams`: independent names/OIDs/lengths/modifiers and exact rows across the 17 logical variants and 29 codec families. Catalog VECTOR membership depends on the current schema. Wider catalog and relational qualification remains #761/#776. |
| TYPE-002 | Scalar/array empty and NULL matrices, `type_json_array_nulls`, `type_json_array_copy` and existing `json_writes` owners: field length −1, literal text NULL, document null, storage and restart. Known direct document-null ARRAY(JSON) elements have the selected 0A000 exclusion. |
| TYPE-003 | `type_codec_boundaries`, scalar matrix and near-code exact-float8 rejection owners: integer endpoints, integers beyond 2^53, literal network bytes, scalar widths and Boolean byte validation. Existing integer SUM/arithmetic owners remain part of the full suite. No exact-decimal or FLOAT4 result type. |
| TYPE-005 | `type_numeric_metadata` and source-seam owners: original 700/1700 OIDs, NULL origin, fixed/constrained outputs, required casts, cache validation, recursive output positions and existing failed-transaction error priority. Unsupported binary adapters remain explicit. |
| TYPE-006 | Scalar/array temporal packets and `type_codec_boundaries`: pre-2000 dates, negative timestamp microseconds, fractional units, TIME endpoints and range rejection. Existing temporal text/binary owners supply the remaining finite parser cases. BC/infinity/24:00 extensions remain excluded. |
| TYPE-007 | Temporal wire/storage matrices and existing canonical-instant comparison/order owners retain UTC timestamp semantics and the documented timezone deviation. Later cross-operator and typed-kernel matrices remain #776 and the dependent operator owners. |
| TYPE-008 | `type_json_scalar_documents`, JSON-array origin/COPY owners and near-code parameter/column carrier owners: exact unsigned document numbers, strings/objects/nested null, SQL NULL, conflict updates, predicates, grouping, forwarding and restart. Ordinary numeric/Boolean carriers retain their current policies. No JSONB ABI or universal JSON predicate/hash equivalence. |
| TYPE-009 | Fourteen-family array matrices, malformed codec boundaries, text-parser and existing row-array owners: dimensions, lower bound, element OID, NULL flags, complete framing and checked count before allocation. Standard array OIDs, arbitrary dimensions, VECTOR-array wire identity and direct document-null elements remain excluded. |
| TYPE-010 | `type_vector_carriers`, codec boundaries and near-code vector owners: finite f32 bits, declared width/header/reserved bytes, maximum binary width 32767, text/binary parameter and stored/RETURNING paths, exact component endpoints and preserved error priority. The width-32768 probe rejects binary output before writes while retaining text NULL output; no storage dimension cap is added. |

The scalar, scalar-array and original declared-OID NULL matrices contain 548
finite exchanges: 150, 224 and 174 respectively. The additional integer/TIME/
Boolean/array-length boundary owners contain 51 exchanges. Exchange counts
describe fixture coverage and are separate from test counts or proof of a
complete SQL/operator/physical-path cross-product.
