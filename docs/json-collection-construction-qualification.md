# JSON collection construction qualification

This source increment belongs to [#811](https://github.com/cntryl/cassie/issues/811)
and shares the selected scalar JSON conversion authority with #810. Integrated
head `86174ceb` executed the original candidate and confirmed 20 JSON aggregate
SQL failures: JSONB results were double quoted after becoming physical rows.
The carrier/timezone correction below and its new probes await the next combined
executable gate. No source review or authored probe is reported as a passing
runtime result.

The private `executor/json_construction.rs` helper supplies the existing scalar
builders and collection finalization with one type-aware conversion rule.
Finite NUMERIC values remain exact JSON numbers, including decimal scale and
integers beyond FLOAT8 precision. Finite REAL values are numbers. Nonfinite
NUMERIC, REAL and FLOAT8 values become JSON strings such as `"NaN"` and
`"Infinity"`. SQL TEXT is quoted; native JSON/JSONB expressions remain documents.
Plain JSON retains original nested document text and duplicate object members;
JSONB applies its existing canonical ordering and last-key rule. JSONB collection
results now retain the native `Value::Json` document carrier in physical rows;
plain JSON keeps its raw `Value::String` text. This prevents source-aware casts
and operators from treating an encoded JSONB aggregate as SQL TEXT. Result OIDs
remain 114 and 3802, including binary JSONB version byte 1.

JSON date and timestamp values use ISO spelling even when the session DateStyle
is SQL, Postgres or German. TIMESTAMPTZ uses the statement TimeZone and JSON's
explicit timezone minutes (`-05:00`), including UTC, historical second offsets,
BC and infinities. SQL datetime formatting keeps its original mode. TIMETZ
retains its PostgreSQL JSON short whole-hour offset (`+05`), and INTERVAL uses
the statement IntervalStyle. JSON object keys retain ordinary
SQL scalar text formatting, including session temporal settings and the native
nonfinite numeric names. These rules are pinned by pure SELECT probes against
the dedicated PostgreSQL 18.6 reference.

Historical-second formatting uses Europe/Paris in 1900 (`+00:09:21`), where
the selected bundled jiff data and PostgreSQL 18.6 agree. The original Amsterdam
witness failed the integrated gate because jiff-tzdb 0.1.9 maps Amsterdam and
Brussels to the same transition data (1900 GMT), while PostgreSQL's database
retains Amsterdam's `+00:19:32` backzone history. The correction preserves the
bundled provider selected by [Extended Types](extended-types.md), retains the
seconds-offset assertion, and does not claim complete pre-1970 PostgreSQL timezone
history parity. Provider bytes, oracle results and the bounded Jev decision are
recorded under `target/v020-evidence/json-timezone-provider/`.

The aggregate retains original values until FILTER, DISTINCT and ordering have
run. Finalization then uses the group's bound logical argument type and its
statement controls. It admits metadata lookup, encoded fragments, directories,
document construction and JSONB canonicalization before allocation. Before the
canonical document becomes a parsed JSONB result, the live workspace covers the
actual entry/string capacities and the locked decoder bound, growing before
parsing if necessary. The final lease retains parsed JSON heap rather than only
encoded text capacity. Cancellation is checked after actual decoding. A private
owned result transfers the retained output reservation into the existing
physical BatchRow owner; output-name/value copies are admitted before cloning.
Backing values retire before their leases on success and error paths.

A private seen-input bit distinguishes zero selected rows from selected rows
whose SQL NULL values a strict aggregate skips. Zero selected rows yield SQL
NULL. All-skipped strict array aggregates produce `[]`; strict object aggregates
produce `{}` (plain JSON spells its empty object `{  }`). Strict JSON and JSONB
array aggregates retain document null. The pinned PostgreSQL 18.6 strict JSONB
object behavior omits document null, while plain JSON object aggregation retains
it. Unique-key ownership includes keys whose values were skipped.

New evidence locations:

- `tests/sql_queries/aggregates/json/typed_conversion.rs`: exact and nonfinite
  values, JSON versus TEXT identity, modifier order, NULL/empty distinctions,
  object keys, and statement temporal formatting.
- `tests/pgwire_extended/aggregates/json_typed_conversion.rs`: Simple Query,
  inferred NUMERIC parameters across text/binary input and result formats,
  native OIDs, late duplicate-key error without rows/completion, and recovery.
- `src/executor/execution/aggregate_exec/collection_json_tests.rs`: the actual
  physical row lifetime, measured just-below/at memory boundaries, and
  cancellation after actual NUMERIC encoding with complete workspace cleanup.
  Carrier probes also cover parsed exact numbers, duplicate plain JSON text,
  cancellation after actual document decoding, and a sparse object with measured
  admission and last-alias ownership.

The ignored `target/v020-evidence/backlog-acceptance/` directory retains oracle
SQL/results, authored-test hashes, bounded Jev questions/answers and the finite
backlog grid. Jev selected contextual finalization and private result ownership;
its ownership leads prompted the physical row lifetime and cleanup controls.
Those judgments do not replace executable qualification.

The original `357babc` source review assigned probability 0.34 to a late-copy
lifetime gap and 0.49 to a missing pre-allocation boundary. The actual later-copy
headroom-failure control and the wide-NUMERIC denial-before-encoder control
are authored focused probes for those leads. Neither is reported as passed;
the complete ordered gate and combined test-inventory regeneration remain
outstanding. The follow-up carrier review selected no concrete failure (0.56),
with ownership risk 0.31 and temporal risk 0.18. The post-decode cancellation
probe and sparse parsed-object admission/lifetime probe address that ownership
lead and remain unexecuted. Oracle and correction artifacts are retained under
`target/v020-evidence/json-aggregate-carrier/`.

A subsequent source-only array increment replaces the JSON-construction call to
`SqlArray::to_nested_json` with a private dimension walker. It preserves row-major
shape through six dimensions, leaves lower bounds and SQL/document NULL origins
unchanged, and reuses scalar conversion with the declared element type. Exact
NUMERIC/REAL values remain numbers; nonfinite numeric values become strings;
JSON document text keeps duplicate members while SQL TEXT is quoted. Temporal
array elements use the statement JSON formatting described above. This applies
to builders, scalar conversions and collection finalization through their shared
helper. The existing workspace is admitted before array text/fragment allocation;
both raw fragments and canonical JSONB output are bounded before allocation;
preflight and actual element encoding check interruption, including a probe
paused after actual first-element work. JSONB still uses its existing canonical
finalizer and parsed physical output owner.

The array source increment has six SQL probes (including paired REAL/FLOAT8
controls), one text/binary parameter/result
matrix and nine native semantic, measured-admission, late-error, cancellation and
physical last-alias probes authored from twelve PostgreSQL 18.6 oracle cases.
They are pending the combined executable gate, not passing evidence. Source,
Jev decision/review and oracle artifacts are retained under
`target/v020-evidence/json-array-construction/`. Jev selected the private encoder
and existing workspace plan (1.00 each); its stored-CHAR (0.41) and late-document
error (0.30) leads prompted focused probes. The stored bounded CHAR[] probe
requires the independently integrated logical-padding hydration work. Persisted
unconstrained bpchar[] cannot reconstruct per-value blanks from canonical trimmed
bytes without a separate specified representation decision (#796/#807); this
increment does not fabricate a width.

The general `SqlArray::to_nested_json`, row encoding, Midge, public value APIs,
dependency versions, CI, release configuration and feature support status remain
unchanged. #810, #811 and their selected prerequisites remain open until their
complete finite acceptance inventories and final gates are resolved.
