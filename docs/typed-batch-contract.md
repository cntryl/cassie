# Internal Typed Batch Contract

This is the selected private specification for [#753](https://github.com/cntryl/cassie/issues/753).
It follows the finite [SQL type contract](type-contract.md) qualified in
[#786](https://github.com/cntryl/cassie/pull/786), merged as
`7ac3f3429e4d38cd8893521f531518f0e7c672ca`. Common typed constructors and
scan/filter/projection execution remain Planned under
[#756](https://github.com/cntryl/cassie/issues/756). Written laws do not establish
runtime support or satisfy their future implementation probes.

This specification selects private in-memory representations only. Midge remains
the direct storage layer. Existing CBM2/row encodings, type tags, PostgreSQL OIDs,
input/output codecs and public interfaces retain their existing authorities.
No public batch API, durable format, migration or native temporal ABI follows
from a buffer choice.

## First transport and operation matrix

Every current logical type remains representable with its exact declared type,
modifiers and admitted carrier. The first native transport set is Boolean,
SmallInt, Int, BigInt, Float, Text, Char and Varchar. Transport eligibility does
not imply that every operation has a native kernel.

| Logical type | First transport | First native operation scope and preservation |
| --- | --- | --- |
| Null | AllNull view and logical metadata; no payload | NULL tests and passthrough; this is an inference carrier, not a declared DDL type. |
| SmallInt | i64 lanes plus SmallInt tag | NULL tests, exact numeric comparisons, passthrough/gather; retain i16 range. |
| Int | i64 lanes plus Int tag | Same scope; retain i32 range. |
| BigInt | i64 lanes plus BigInt tag | Same scope; no f64 intermediate, including integers beyond 2^53. |
| Float | f64 lanes plus Float tag | NULL tests, existing exact mixed numeric comparisons, passthrough/gather; preserve source bits. |
| Boolean | Canonical u8 lanes, each 0 or 1, plus Boolean tag | NULL tests, NOT/AND/OR with three-valued logic, TRUE-only selection, passthrough/gather. |
| Text | UTF-8 bytes and checked usize offsets plus Text tag | NULL tests and passthrough/gather; string comparison/key operations use the shared bounded scalar adapter. |
| Char | Same UTF-8 transport plus exact Char(length) | Same scope; preserve known versus unknown character bounds and current canonicalization boundaries. |
| Varchar | Same UTF-8 transport plus exact Varchar(length) | Same scope; None is unlimited logical character length, subject to query resource limits. |
| Uuid | Schema-tagged ScalarBacked current Value | NULL tests and lossless passthrough/gather; preserve canonical UUID and existing codecs. |
| Bytea | Schema-tagged ScalarBacked current Value | Same scope; preserve exact bytes and the current canonical hex carrier. |
| Date | Schema-tagged ScalarBacked current Value | Same scope; preserve current calendar domain, text and errors; no new epoch. |
| Time | Schema-tagged ScalarBacked current Value | Same scope; preserve current microsecond precision/day domain and text. |
| Timestamp | Schema-tagged ScalarBacked current Value | Same scope; preserve current instant, precision, text and comparison semantics; no new timezone ABI. |
| Vector(n) | Schema-tagged ScalarBacked admitted native, JSON-array or text carrier | Same scope; retain origin until its existing named normalization/output boundary, exact finite f32 meaning and declared dimension. |
| Json | Schema-tagged ScalarBacked admitted primitive or document carrier | Same scope; preserve document NULL, unsigned values and aggregate error identity separately from SQL NULL. |
| Array(T) | Schema-tagged ScalarBacked decoded array plus complete recursive element DataType | Same scope; preserve ordered elements, exact numbers, SQL NULL slots and nested character modifiers without serialization/reparse. |

Arithmetic, casts, functions, CASE and unsupported native comparisons use a named
bounded scalar-expression adapter until individually qualified. Eligible native
operators retain typed buffers between operators; this adapter is a local
semantic boundary, not mandatory JSON or BatchRow reconstruction at every step.
Row serialization belongs to the named final row/wire output boundary.

The private choices favor simple first-kernel access: shared i64 integer payloads,
canonical u8 Boolean payloads, checked usize indices/offsets and immutable owners
holding admitted Vec backing. Narrow integer lanes, packed Boolean payloads and
different owner allocations are later optimizations requiring equivalent laws,
complete accounting and measured benefit. Encoded Boolean bytes/bits are never
reinterpreted as the native u8 layout.

## Logical domain, schema and row correspondence

A batch owns an ordered logical schema, accessible domain length n, emitted
length m and one logical-to-domain mapping S. Identity selection means m = n.
An explicit S has exactly m entries, each less than n; order and repeated entries
are preserved. Every exposed column has logical length m even when its physical
constant or dictionary payload has a different length. Schema identity and full
type modifiers survive empty and all-NULL batches.

Construction requires schema arity equal to column count, with each column's
exact logical type matching its ordered descriptor. Repeated output names are
allowed. Before batch selection, every base column view has domain length n;
matching only the emitted length m cannot excuse an undersized base view.
Check n, m, every view domain and S independently before exposing the batch.

Explicit counts distinguish zero columns with one row from zero rows. Position
descriptors express query-local correspondence, not stored `_id` or a declared
`id`. Filtering, joins, grouping and reordering explicitly construct their new
counts/mappings; none inherits a predecessor count by assumption.

For base values [10, 20, 30] and S = [2, 0, 2, 1], passthrough produces
[30, 10, 30, 20]. A filter examines emitted lanes and appends their existing
domain positions only when the predicate is TRUE, preserving repeats and order.
Computed projection evaluates exactly the m emitted lanes and rebases its output
to domain m with identity selection. Passthrough uses Gather(base, S), where base
is the pre-selection domain accessor. Applying S to an already-selected logical
view a second time is invalid. Parent Slice/Gather mappings compose internally.
Every projected column and its row correspondence expose the same rebased m.

## Validity and representation laws

SQL validity is separate from selection and payload. AllValid, AllNull and an
owned packed Vec<u64> bitmap are the selected validity forms. A root bitmap has
explicit bit count b and exactly checked ceil(b / 64) logical words. A view has
bit base and domain n, with checked bit_base + n <= b. A slice retains its owner
rather than pretending the larger backing has the slice length. Padding bits are
not lanes. Capacities are charged independently of logical word count.

A valid empty string differs from SQL NULL. JSON document NULL is a valid JSON
lane. Invalid lanes are never evaluated by a semantic expression/value kernel.
That rule does not skip physical construction validation: framing, offsets,
codes and canonical native payloads must be valid even for NULL or unselected
rows. Dictionary row and entry validity belong to different domains and compose
by logical AND. A NULL row with an out-of-range dictionary code is malformed,
not a shortcut to a valid NULL result.

| View | Required construction and accessor laws |
| --- | --- |
| Flat fixed | Payload has n lanes; validate declared range and Boolean 0/1 values before publication. Read value payload only for valid selected lanes. |
| Flat UTF-8 | Exactly n + 1 offsets; initial offset 0, monotone offsets, final offset equals payload length. Every endpoint is in bounds and on a UTF-8 boundary. Check all length/capacity arithmetic. Empty spans remain valid empty strings. |
| Constant | One immutable typed value or explicit NULL state, plus domain n. Domain zero has no accessible lane; all-NULL needs no value payload. |
| Dictionary | n codes and an entry view of d lanes. Validate required framing and every code, including unselected or NULL rows. Access maps lane to selected domain position to code to entry. |
| Sequence | Integer-only i64 start and step plus exact integer tag. Use checked i128 arithmetic and checked conversion to the declared range across the entire accessible domain, before selection. Domain zero has no arithmetic lane; one-row domain checks start. Float/temporal sequences do not follow from this selection. |
| ValidatedEncoded | Retain immutable bytes, exact logical schema and validated framing, generation, coverage, code and validity metadata. Use checked access/selected-lane decode; never cast borrowed encoded bytes into native Rust values or let coarse storage tags replace SQL types. |
| Slice/Gather | Check slice base/length and gather codes against the parent domain; retain parent and mapping owners. Parent drop or producer advance cannot mutate retained views. |

Dictionary deduplication uses physical value identity, not semantic-key equality:
projection must retain Float signed zero/NaN payload bits and original string
spelling. No obligatory flattening, SIMD alignment or unsafe byte reinterpretation
is selected. Portable Rust element alignment is sufficient for the first path.

Empty, one-row, full and tail batches obey the same constructors. Reject malformed
schema/length/selection, undersized or oversized validity word vectors, out-of-range
codes, offset overflow, invalid UTF-8 endpoints and sequence overflow before a
view is exposed. Validation and decoding scratch are themselves admitted before
allocation; construction never publishes a partially valid batch.

For example, n=3, m=2 and S=[2,0] reject a base view of length 2 even if its
exposed length would be 2. A typed dictionary with n=0, no codes and d=0 is
valid and never accesses an entry. BigInt Sequence(start=i64::MAX-1, step=1,
n=3) is rejected before publication even if selection contains only lane 0.
Dictionary(n=3, d=2, codes=[0,2,1]) is rejected even if row 1 is NULL and
unselected. These are written constructor examples, not executed runtime probes.

## Logical fidelity and capability authority

A single private capability decision takes the operation, ordered exact logical
operand/result types and view capabilities. It returns NativeTyped with a named
qualified kernel, BoundedScalarExpression with a named shared adapter, or
ExistingUnsupported with the current error boundary. Planner eligibility and
runtime dispatch consume this same authority. Static Boolean/type validation
precedes lane evaluation; native fallback cannot bypass it.

Mixed numeric comparison reuses `types/semantic.rs` and `executor/semantic.rs`.
Do not replace it with lossy int64-to-f64 conversion, raw Float equality or
total ordering of original Float bits. Semantic keys can unify signed zero and
exact integral floats, and canonicalize NaNs; transport preserves original bits.
UTF-8 transport does not authorize a universal byte comparator: current semantic
keys normalize timestamp-shaped text and ARRAY ordering has its own shared owner.
Preserve existing CASE branch demand and errors without introducing an AND/OR
left-to-right evaluation promise.

Specific consequences of the qualified type contract are mandatory:

- Standalone Float transport preserves existing permitted query NaN/infinities
  and signed zero. Finite FLOAT-array text admission and finite vector admission
  remain their named boundaries; JSON-backed writes keep their existing rejection.
- Decoded numeric arrays retain exact BIGINT values and f64 bits directly. A
  default-serde serialization/reparse can lose repaired input precision and is
  not a typed conversion. Lexical integer -0 is already normalized to integer 0;
  this does not erase f64 signed zero.
- Char(None) wire input retains raw Unicode text and trailing ASCII spaces until
  a known target supplies its bound. Known CHAR(n) retains character-count
  validation and current unpadded canonicalization. Bare SQL CHAR means CHAR(1).
  These boundaries do not justify trimming TEXT/VARCHAR transport.
- Varchar(None) is unlimited logical character length; Some(n) is a character
  bound rather than a byte bound. Inner ARRAY modifiers remain in the DataType
  tree even when wire typmod is -1.
- ScalarBacked preserves admitted Value/carrier provenance. VECTOR can arrive
  through native, JSON-array RETURNING or text Bind carriers; a blanket bridge
  or mandatory Vec<f32> normalization would change existing output semantics.
- JSON primitives and rich documents retain their current carrier split.
  Non-numeric JSON SUM/AVG errors remain JSON errors, not incidental TEXT errors.
  SQL NULL slots, direct ARRAY(JSON) document-null element exclusion, nested JSON
  document support and VECTOR-array wire exclusion retain the type contract.
- Declared descriptors and command-owned output metadata survive physical buffer
  choices, including unknown 705 and input-only numeric 700/1700 adapters.

## Ownership, admission and terminal cleanup

Views share a private Arc owner that is immutable after publication and holds
its Vec backing and reservation. Moving already-admitted backing transfers its
capacity and reservation together. Shared backing is charged once by its owner;
newly allocated/copied backing and previously unadmitted spare capacity require
independent admission. Retain source guards through conversion/output handoff.

Charge actual capacities, nested Value/type heaps, UTF-8 bytes and offsets,
validity/maps, dictionary backing, new schema/name copies, Arc/owner metadata and
old-plus-new resize/conversion overlap. Newly query-owned descriptors are charged;
existing shared planning descriptors remain in their documented boundary.
Compute checked retained-capacity charges and acquire reservations before any
new backing, copy, growth or owned validation scratch allocation. During growth
or conversion retain old backing and its reservation while admitting the full
new capacity. Publish the new owner only after successful construction; failed
admission or validation drops only provisional state and leaves existing views
and their reservations intact. There is no allocate-then-charge path.
Reservations outlive backing destruction and the last view, including when a
parent handle is dropped or a producer advances. Cancellation, deadlines and
budget errors expose no partial batch or native-success diagnostic. Final
consumer destruction releases the owned memory and workers.

For an admitted source buffer with capacity 128 and two aliases, source backing
is charged once; an owned selection map with capacity 8 has its own charge.
Dropping the parent and first alias retains the source charge and accounting
authority through the last alias. A failed copy admission releases neither
existing backing nor its lease. On last-alias destruction free backing before
releasing its reservation. These are constructor/lifetime laws for #756 probes,
not measurements or current runtime acceptance.

## Finite specification acceptance and future probes

Each assigned #753 owner is selected as a documented boundary. Runtime constructor,
accessor, ownership and capability proof remains #756 work; the entries below are
required future probes, not test results. Existing mapped scalar tests remain
scalar evidence and do not prove a common typed path.

| #753 owner | Selected written boundary | Required #756 implementation witness |
| --- | --- | --- |
| PIPE-01 | Equal logical lengths/schema/correspondence, explicit zero-column count, operator count changes | Empty/one/full/tail and split-boundary pipelines, checked per-batch metadata. |
| VEX-01 | Ordered schema, n/m domain and malformed input rejection | Mismatched lengths/schema/selection and typed empty/all-NULL output. |
| VEX-02 | Validity separate from ordered selection and dictionary domains | Sparse NULLs with S = [2,0,2,1], dictionary NULLs, unselected invalid codes and sliced bitmaps. |
| VEX-03 | All 17 logical families preserved, finite native subset, unchanged codec exclusions | i64 extrema and 9007199254740993; Float signed zero/NaN/subnormal; parameterized and all-NULL types; decoded numeric-array fidelity. |
| VEX-05 | Seven view forms expose the same logical accessor without mandatory flattening | Flat/constant/dictionary/sequence/encoded/slice/gather equivalence, overflow and demanded-lane behavior. |
| VEX-06 | Checked variable-width offsets, immutable owners and admitted lifetime | Invalid UTF-8 offsets, empty versus NULL, parent drop/producer advance, source-fit/map-budget failure and final zero-owned-resource counters. |
| VEX-26 | One operation/type/view capability authority with shared scalar semantics | Planner/kernel decision agreement, forced native decline, rebind identity, exact scalar/typed bag and error equivalence. |

Neither this specification nor the future runtime tests promote the broader
relational kernel, portal, storage/recovery or production-readiness owners.

## First runtime implementation coverage

The #756/#757 bundle implements the selected transport/view constructors before
connecting scan, filter and projection. The implementation and its acceptance
remain pending until named runtime witnesses and exact-revision validation are
recorded. The selected boundaries are:

| Boundary | First implementation scope | Acceptance owner |
| --- | --- | --- |
| Storage to typed scan | Direct controlled Midge/session inputs; preserve current row/column decoding and source leases. Existing encoded JSON decoding is a storage conversion, not a zero-copy native column claim. | #756 constructor, identity, framing and staged-visibility probes. |
| Typed filter | Numeric comparisons using shared exact numeric semantics, Boolean three-valued operators and NULL tests; TRUE-only stable selection. | #756 sparse validity, repeated positions and split-size differential probes. |
| Typed projection | Shared passthrough/gather with exact descriptors, names, order and row correspondence. | #756 parent-drop, aliases, selection rebasing and lossless type probes. |
| Tight query-memory profile | Before source opening, retain existing scalar dispatch when available memory is below one preferred-batch Value carrier vector. | #756 shared-owner threshold, first-wide-row rejection and LIMIT/EXISTS probes; admission remains enforced. |
| Bounded scalar expression | Existing arithmetic, casts, functions, CASE and other unsupported native operations retain their scalar error/demand rules. | #756 guarded CASE, exact carrier and fallback equivalence probes. |
| Final rows and wire | Explicit admitted output conversion; portal snapshots and completion remain governed by the selected wire transaction contract. | #757 WIRE-017 through WIRE-021. |
| Other physical operators | Existing indexed, ordered, aggregate, join and specialized paths retain their own selected boundaries until their dependent kernel owners complete. | #758 through #780, as assigned in the ownership ledger. |

Native execution cannot be inferred merely from an encoded-index hit, a batched
scalar loop or a passing result comparison. Acceptance must identify the actual
representation, selected kernel/fallback boundary, reads/decodes and final lease
cleanup. This section selects implementation coverage; it records no runtime pass
or support promotion.

The private [typed aggregate contract](typed-aggregate-contract.md) selects
streamed ungrouped numeric aggregates, lawful bounded worker merges and primitive
CBC2 numeric predicate/aggregate handoffs under #758. Grouped, DISTINCT output,
expression and other excluded combinations retain their documented existing
boundaries. The contract records each assigned invariant and its focused probes.

Common encoded transport reuses existing metadata/generation, row-ID and complete
field-frame validators and retains immutable source bytes with admitted metadata.
Numeric CBC2 fields now expose primitive nullable decoded owners through the
common accessor; other fields keep the prior admitted decode-cache conversion.
This is decoded primitive execution. Selected numeric aggregate predicates use
admitted logical selection; other filtered or bounded reads keep existing encoded
pruning and selected-field decoding. Staged changes use the controlled session
source instead of trusting a persisted index. Kernel eligibility and diagnostics
remain specific to the selected operation and representation.
