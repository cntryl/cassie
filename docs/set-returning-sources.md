# Selected array and series table sources

This is the finite #808 contract for Cassie's read-model query sources. It uses
existing selected types and the existing Midge 0.5.1 storage interface. It adds
no storage abstraction, layout, durable type representation or public batch API.
The existing array shape and wire identities remain governed by the
[Type Contract](type-contract.md).

The user selected all series signatures below on 2026-11-04. Runtime delivery and
qualification remain pending under #808; this contract is not a support claim.

## UNNEST and ordinality

`unnest(array)` emits scalar elements in row-major storage order, independently
of an array's lower bounds. SQL NULL arrays and empty arrays emit zero rows;
NULL elements emit rows containing SQL NULL. Multiple array arguments advance
together to the longest input, padding exhausted or NULL inputs with SQL NULL.
Only currently selected array element families are included.

Native ARRAY constructors follow PostgreSQL18.6 `ExecEvalArrayExpr`: up to six
dimensions; all NULL/empty subarrays produce a zero-dimensional empty array;
mixing empty/NULL subarrays with nonempty subarrays fails `2202E`. This is an
SQL construction rule over the existing shape carrier, with no row-layout change.
Array-valued `ARRAY(SELECT ...)` accumulation follows the separate native
`accumArrayResultArr` rule: NULL subarrays fail `22004`, empty or mismatched
shape/lower bounds fail `2202E`, and adding a dimension beyond six fails `54000`.
Array rank does not create a different element-family array OID.

`WITH ORDINALITY` appends a non-NULL BIGINT column named `ordinality`, starting
at one. It counts emitted rows, including rows containing NULL elements. It
resets for each invocation, including each outer row of a lateral join.

The default data-column label is `unnest` for each argument. Duplicate default
labels remain duplicate labels; references must resolve unambiguously. Ordinary
single-output relation aliases also supply that data column's default label,
as in `unnest(ARRAY[1]) AS x` producing column `x`.
Relation aliases and prefix column-alias lists rename outputs by position,
including ordinality. Excess aliases fail with `42P10`. Function arguments can
reference preceding FROM sources without spelling LATERAL. LEFT lateral joins
preserve an outer row when its source emits no matching rows.

## Selected generate_series signatures

| Resolved signature | Result OID | Default step |
| --- | --- | --- |
| `generate_series(int4, int4 [, int4])` | 23 | INT4 one |
| `generate_series(int8, int8 [, int8])` | 20 | INT8 one |
| `generate_series(numeric, numeric [, numeric])` | 1700 | exact NUMERIC one |
| `generate_series(timestamp, timestamp, interval)` | 1114 | none |
| `generate_series(timestamptz, timestamptz, interval [, text])` | 1184 | none |

These are resolved signatures. Existing permitted implicit input conversions
may select one of them; in particular, a fourth timezone argument selects the
TIMESTAMPTZ signature even when a TIMESTAMP input can be converted to it.
Supplied parameter OIDs retain their input identity. Inferred parameters use the
selected signature's input identity; wholly unspecified ambiguous signatures
fail with `42725`. Unsupported concrete signatures, including FLOAT4/FLOAT8
series, fail with `42883`. Result typmods are -1. Exact NUMERIC values retain
their output scale. No new logical type or wire identity is selected.

Series bounds are inclusive. The step sign selects ascending or descending
comparison; an opposing bound direction emits no rows. SQL NULL in any argument,
including an explicit step or timezone, emits zero rows. Signature binding still
runs, but a NULL invocation does not enter step or timezone validation.

A non-NULL zero integer, NUMERIC or interval step fails with `22023`. Interval
sign and zero use PostgreSQL's existing 30-day-month interval comparison, so
`INTERVAL '1 month -30 days'` is a zero step. NaN and infinite NUMERIC start,
stop or step arguments fail with `22023`; infinite interval steps also fail
with `22023`. Named zones use the selected temporal contract and invalid zones
retain its `22023` diagnostic.

Finite TIMESTAMP/TIMESTAMPTZ bounds are selected. Infinite temporal bounds fail
with `0A000` as outside this finite source profile. PostgreSQL 18.6 accepts these
bounds in its generator implementation; its FunctionScan materialization is not
Cassie's resource contract, and the bounded infinity probe timed out rather
than producing an accepted value receipt. This boundary does not change general
timestamp infinity support elsewhere in Cassie.

Integer successor overflow ends iteration after the last admitted value, as in
PostgreSQL's INT4/INT8 generators. Exact NUMERIC advancement uses exact addition
and comparison; an intermediate successor beyond the stop is not emitted.
Temporal advancement uses the existing checked calendar interval operation.
Its native range error is `22008`, including a successor formation error at the
maximum timestamp date. Ordinary type conversion and arithmetic diagnostics
retain their existing SQLSTATEs.

TIMESTAMP steps follow the existing civil calendar. TIMESTAMPTZ calendar steps
use the supplied zone or the statement's session TimeZone. Calendar days and
months preserve the selected local-time/DST law; interval microseconds remain
elapsed time. No fixed-duration replacement for calendar stepping is selected.
The default output label is `generate_series`; aliasing, implicit lateral scope
and WITH ORDINALITY follow the rules above.

## Consumption, cancellation and memory

Ordinary source/filter/projection/LIMIT execution pulls rows as needed. A WHERE
predicate with LIMIT must stop generation once enough qualifying rows have been
produced. LIMIT zero does not invoke the source. Aliases and lateral source
composition must preserve this pull boundary. Blocking sort, grouping, window,
DISTINCT or explicitly materialized query boundaries may collect admitted rows.
Schema-backed collection inputs use the existing Cassie `SessionRowCursor`
transport one row at a time; the current published Midge0.5.1 API remains fixed.

Do not allocate a complete expanded row set before the consumer asks for it.
Admit retained arguments and row buffers before growth, check cancellation at
advancement and consumer boundaries, and release source/input/output ownership
on exhaustion, early LIMIT, cancellation and error. Query deadlines remain in
force for calendar steps or predicates that do not quickly reach a stop.
A private expression scratch owner retains admitted nested array-function
intermediates until argument evaluation completes; input ownership transfers to
the source cursor before that scratch is released.
Native JSONB array elements use the existing canonical JSONB formatter and exact
numeric expansion; JSON elements retain their selected JSON carrier semantics.
Native array text publication and explicit array-to-TEXT casts use the captured
statement TimeZone, DateStyle and IntervalStyle. A private result context carries
that snapshot through pgwire formatting; public Value/SqlArray carriers and
native binary values retain their existing representation.
A terminal error returns no partial result rows or command tag; a subsequent
statement on the same connection must recover according to its transaction state.

## Unselected neighboring forms

This selection does not include `generate_subscripts`, generic `ROWS FROM`,
projection-list SRF expansion, new array element families or arbitrary extension
functions. Their former gap-analysis grouping under #808 was planning inventory,
not selected acceptance. They require a separate read-model scope decision and
are not counted as completed by this issue.

PostgreSQL 18.6 values, errors and descriptors are the semantic reference for
these selected forms. Its eager FunctionScan materialization, spilling and
broader PostgreSQL function surface are not selected Cassie behavior.
