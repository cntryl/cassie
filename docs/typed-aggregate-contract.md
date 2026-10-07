# Typed aggregate contract

This private runtime slice belongs to #758. Its selected aggregate subset is
COUNT(*), COUNT/SUM/AVG/MIN/MAX of unqualified SMALLINT, INT, BIGINT and FLOAT
columns in one ordinary collection. Grouped, DISTINCT output, expression aggregates,
HAVING, ordered/paginated aggregate output, CTEs, sets and correlated outer scopes
retain their existing scalar or encoded aggregate dispatch. Midge remains the
only storage layer; no public API or persistent encoding is introduced.

Typed accessors consume selected valid lanes in logical order. COUNT emits BIGINT
and ignores SQL NULL except COUNT(*). Empty/all-NULL COUNT is zero; other functions
emit NULL. SUM retains exact integer values above2^53. Integer partials retain
running prefix minima/maxima and append those ranges in original partition order:
[ MAX ] plus [ 1,-1 ] must fail, and [ -MAX ] plus [ MAX,MAX ] must succeed even
when the latter partition overflows locally. Finishing an isolated partial before
ordered merging is forbidden.

The legal bounded-worker merge subset is COUNT, integer SUM and numeric MIN/MAX.
Equal extrema retain the first logical carrier, including signed zero. AVG and
FLOAT SUM use the existing sequential fold and overflow authority; their chosen
row-order fallback never silently reassociates floating additions. Worker counts
and segment/batch splits cannot alter those values, errors or declared types.

Partial state is bounded by aggregate width and admitted active worker count;
source batches and worker/output owner leases remain controlled. Worker results
merge in partition order. Cancellation/deadline checks run while consuming lanes
and before publication. No partial rows or aggregate success metrics publish on
failure. Existing aggregate/worker diagnostics identify chosen completed paths;
encoded-direct diagnostics only describe genuinely validated encoded inputs.

Existing summary/direct encoded aggregation remains first dispatch. Common typed
scan can supply validated numeric encoded owners without converting each lane to
JSON. Codec framing, validity and selection must be checked before consumption.
Unsupported string/JSON/expression codecs and unsafe fold combinations retain
controlled existing fallback. Encoded numeric selection, representation/batch/
worker equivalence and live-owner cleanup are mandatory witnesses for this slice.

PIPE-06 keeps native grouped/expression aggregation outside the selected boundary;
EXEC-07 owns BIGINT COUNT and NULL identities; EXEC-08 owns exact integer precision
and serial prefix overflow; EXEC-09 owns FLOAT/AVG row-order folding; VEX-12 and
VEX-17 keep typed numeric consumption/retention finite and observable. Final
acceptance requires probed dispositions for all six, adversarial review, complete
repository validation and exact final revision evidence. #758 stays open while
any selected obligation or validation gate remains pending.

The selected native encoded handoff decodes numeric CBC2 Plain, Constant, RLE,
Dictionary, integer FOR and floating ALP into admitted primitive nullable owners.
It reuses the existing frame/validity/bitpacking/scaling rules and compares the
expected segment domain before allocating decoded lanes. Numeric aggregate cells
borrow those owners directly; JSON/value carriers remain only at the existing
nonnumeric or scalar fallback boundary. Numeric native handoff metrics must record
zero prematurely materialized lane objects, while retaining raw framing and owner
leases for the complete typed batch lifetime.

The selected predicate handoff supports unqualified numeric column/literal
comparisons, IS NULL, and Boolean AND/OR/NOT over that subset. Parameterized,
string, arithmetic, function and correlated predicates retain existing fallback.
Predicate-only numeric columns join the admitted input schema. A native predicate
produces an admitted logical selection and feeds aggregate cells directly; it
does not introduce a JSON/value lane boundary. This filtered subset runs before
legacy filtered encoded aggregation, while unfiltered summaries keep first
priority. Typed encoded scans retain their existing truthful column-batch scan
counters; aggregate completion uses worker diagnostics and never increments the
legacy direct path's zero-row-id counters, because typed scans retain row IDs.

Aggregate DISTINCT syntax retains its existing parser rejection; this slice does
not promote it. SELECT DISTINCT aggregate output uses the existing scalar phase.

## Finite invariant dispositions

| Invariant | Selected disposition and executable evidence |
| --- | --- |
| PIPE-06 | Ungrouped numeric bare-column aggregates and finite numeric predicates are implemented. `should_handoff_encoded_numeric_predicates_to_typed_aggregates_without_objects` checks the real typed API and public SQL, exact values, COUNT OID20, and truthful scan/direct counters. `should_keep_excluded_aggregate_combinations_on_existing_fallback` probes grouped, SELECT DISTINCT output, expression, string and parameter predicate dispatch separately. Aggregate DISTINCT parser syntax remains unsupported. |
| EXEC-07 | BIGINT COUNT and empty/all-NULL identities are implemented. `should_stream_numeric_aggregate_results_and_release_source_and_output_owners`, `should_match_scalar_aggregate_values_and_errors_for_selected_numeric_carriers`, and `should_retain_empty_and_all_null_encoded_aggregate_identities_and_count_types` compare exact identities and output OIDs. Existing unfiltered summary/direct dispatch remains authoritative before typed dispatch. |
| EXEC-08 | Exact integer cells and translated ordered prefix ranges are implemented. Every typed batch/two-way partial split probes MAX,1,-1 and MIN,-1,1 overflow and globally legal -MAX,MAX,MAX. Production worker overflow releases owners without success publication; beyond2^53 encoded predicate results retain exact SUM=1. |
| EXEC-09 | FLOAT SUM and every AVG explicitly use the serial fold. `should_preserve_float_row_order_and_nonfinite_overflow_across_typed_splits` compares exact outcomes at every split; `should_preserve_float_row_folds_and_carriers_through_encoded_numeric_owners` compares row and encoded cancellation values at worker settings1/4; numeric-codec witnesses compare errors and row-order results without tolerances. |
| VEX-12 | COUNT, integer SUM and numeric extrema merge bounded width states in contiguous partition order under admitted workers. `should_merge_bounded_typed_workers_and_publish_only_completed_aggregation` exercises worker settings1/2/4 over multiple waves. Ordered partial, overflow, cancellation-after-completed-wave and transport-floor probes establish lawful merges, complete cleanup and success-only diagnostics. Floating reassociation is excluded. |
| VEX-17 | Native numeric CBC2 Plain/Constant/RLE/Dictionary/FOR/ALP handoff, validity and selection are implemented. Primitive decoder goldens and `should_match_row_values_errors_and_workers_through_every_selected_numeric_codec` establish exact row/encoded results, errors and actual encoded COUNT/extrema worker execution. The scan parent-drop probe asserts zero JSON numeric lanes with retained source ownership; filtered public SQL asserts zero materialization. FSST strings and other nonnumeric combinations keep existing controlled boundaries, exercised by the string-predicate fallback. |

Tests live in `src/executor/execution/aggregate_exec/typed/{tests,encoded_tests}.rs`,
`src/midge/adapter/column_batch_format_v2/numeric_tests.rs` and the retained encoded
scan witness in `src/executor/scan/typed.rs`. The ignored target evidence records
exact failing and passing commands and immutable source hashes. These are focused
local checks; the coordinator must still run build, locked full tests, full
workspace/all-target/all-feature pedantic clippy, fmt check and touched-test
validation on the integrated revision before closure. There is no performance or
compressed zero-copy promotion in this slice.

Native numeric decode admission covers raw bytes, metadata and at least64bytes per
expected row for primitive buffers and peak temporaries, in addition to existing
conservative decoded-byte admission. Header row count must match the expected
segment before allocating lanes. Dictionary cardinality and RLE runs cannot
exceed the non-NULL count, including zero; each numeric entry has eight validated
scalar bytes. FOR uses at most128-lane unpack temporaries. Current ALP has no
exception array: block count equals the expected128-row partitioning, width is
bounded64 and exact packed lengths/ranges are checked by existing code.

`should_preserve_own_session_overlays_and_other_session_encoded_aggregate_visibility`
checks that active changes use the controlled row source while another session
can still consume the committed encoded owners.

Plain and dictionary scalar buffers use explicit expected-count capacities.
This avoids Result-iterator minimum-capacity growth for tiny domains and makes
the primitive temporary bound independent of pending field-owner headroom.
