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

Native array-to-JSON element formatting, including canonical stored CHAR padding,
remains a separate selected correspondence obligation with #807. This increment
does not change `SqlArray::to_nested_json`, row encoding, Midge, public value APIs,
dependency versions, CI, release configuration or feature support status. #810,
#811 and their selected prerequisites remain open until their complete finite
acceptance inventories and final gates are resolved.
