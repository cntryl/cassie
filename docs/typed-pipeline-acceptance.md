# First typed scan, filter and projection acceptance

This record defines the finite #756/#757 witness set under
[the private batch contract](typed-batch-contract.md) and
[the bounded wire contract](pgwire-transaction-contract.md).
Validation results and the immutable tested revision belong to the linked PR
review record. A named witness is evidence for its exercised case, not proof of
all possible interleavings or later relational kernels.

## Runtime boundaries

The controlled row cursor produces admitted typed columns directly. Unfiltered
covered CBC2 projections retain validated immutable encoded bytes and metadata
alongside their decoded typed accessor. CBC2 decoding remains a storage
conversion; it is not zero-copy native decoding. Filtered CBC2 execution retains
its existing pruning, complete framing and late selected decode path.

Typed selection survives filter and projection. Numeric comparisons, Boolean
three-valued logic, NULL tests and passthrough use the selected native accessors.
Other supported expressions use the existing evaluator through an admitted
one-lane scalar boundary. Function calls and EXISTS retain existing query dispatch
where the plan needs those contexts. Runtime CASE evaluates demanded selected lanes; ordinary
AND/OR evaluation order, constant folding and aggregate-before-CASE behavior
retain their existing SQL contract.

Final output converts typed cells to admitted rows for existing wire serialization.
Joins, ordering, aggregation, CTEs, views and specialized access paths retain
explicit existing execution boundaries. Configured parallel scans retain their
existing worker dispatch. Staged writes use the controlled session cursor rather
than stale encoded acceleration. Native kernel selection is decided by the
operation/type capability classifier; an EXPLAIN mode name or column-index hit
alone does not prove a native operator.

Typed transport requires available query memory for one preferred-batch carrier
vector (`DEFAULT_BATCH_SIZE * size_of::<Value>()`). Below that conservative
capability floor, dispatch selects the existing scalar cursor before typed
metadata admission or storage reads. Already retained query owners reduce the
available budget. This preserves tight-profile first-row rejection and LIMIT/
EXISTS early-stop behavior without retrying a partially read source or weakening
memory admission. `should_select_typed_transport_at_the_available_carrier_budget_boundary`
exercises the shared-owner threshold and release behavior.

CBC2 projections with filters, LIMIT or OFFSET retain the existing bounded
reader. Unbounded encoded transport checks metadata and total retained source
memory before opening field data; insufficient transport capacity selects the
existing reader. Field-read and subsequent operator admission failures remain
terminal. The encoded projection tests in
`tests/query_scan_controls/column_limit.rs` cover limited and complete results
under a one MiB budget and verify that retained query memory is released.

## #756 witnesses

| Invariant | Selected witness and boundary |
| --- | --- |
| PIPE-02 | `should_retain_validated_encoded_backing_after_advancing_the_scan`: encoded bytes and admitted metadata survive a derived slice and are released by its final drop; read counters remain observable. |
| PIPE-03 | `should_match_scalar_bag_across_selected_batch_boundaries`: scalar comparison at 0, 1, 7, 64, 1023, 1024 and 1025; `should_preserve_filtered_projection_across_multiple_batches`. |
| PIPE-04 | `should_apply_selection_once_through_filtered_projection`, `should_keep_selected_text_backing_admitted_until_the_last_alias_drops`; existing owned row projection remains a fallback. |
| PIPE-07 | Existing `should_apply_encoded_conjunctions_with_null_predicates_exactly`; filtered ALP/CBC2 retains the established selected-decode boundary. |
| COL-03 | `should_roundtrip_sparse_validity_with_signed_extremes` and typed sparse/repeated BIGINT selection. |
| COL-04 | `should_validate_bounded_row_id_chunk_roundtrips`; encoded source reuses the existing row-ID loader and validators. |
| COL-05 | `should_return_declared_id_when_column_index_covers_only_other_fields`; internal identity references use the controlled row cursor. |
| COL-06 | `should_emit_declared_type_for_column_readded_with_a_different_type`; live schema controls SQL descriptors. |
| COL-12 | `should_late_materialize_selected_values_despite_unrequested_corruption`; filtered encoded scans retain their existing path. |
| COL-13 | `should_keep_case_distinct_columns_in_encoded_scans`; source matching uses the shared identifier path. |
| COL-14 | `should_prune_column_batch_segments_for_range_filter`; filtered encoding remains the existing pruning owner. |
| COL-15 | `should_fallback_when_any_segment_is_missing_and_rebuild_after_restart`; required encoded input is validated before any source is published. |
| EXEC-01 | `should_discard_typed_output_when_a_later_storage_page_is_corrupt`: no partial result and zero remaining reservations. |
| EXEC-02 | Existing `should_preserve_nested_outer_join_shapes_in_each_execution_mode`; typed join kernels remain outside this slice. |
| EXEC-03 | Existing `should_apply_residual_predicates_before_a_scalar_index_limit`; typed source sizing additionally preserves LIMIT, EXISTS and result-cap early stop. |
| VEX-04 | `should_use_bounded_case_fallback_only_for_demanded_selected_lanes`; existing CASE tests retain casts, constant folding and aggregate boundaries. |
| VEX-07 | Private `capability::expression` selects the native subset or bounded scalar expression boundary; unsupported source shapes retain existing execution. |
| VEX-08 | `should_match_scalar_dispatch_for_supported_predicates`; shared binder contextual typing precedes execution. |
| VEX-09 | `should_preserve_typed_transport_identity_through_repeated_projection`, parameter type inference and alias-copy admission. Views/CTEs retain existing execution. |
| VEX-16 | Direct projected historical row decode plus existing filtered CBC2 selected decode; streaming LIMIT/EXISTS/result-cap tests verify visit bounds. |
| VEX-18 | Scalar bag equivalence across empty and preferred batch boundaries, with sparse NULLs and repeated aliased projection. Ordering remains outside this unordered slice. |
| VEX-23 | Direct Midge-to-typed scan and typed filter/projection witnesses; explicit admitted output conversion. Later relational operators keep their named fallback boundaries. |
| VEX-24 | Typed staged portal witness plus existing mutation/RETURNING tests; staged cursor owns the immutable overlay and mutations retain existing statement atomicity. |

Constructors additionally exercise the inherited private batch laws: mismatched
lengths, repeated selection, zero-column tuples, sparse validity, all 17 logical
families, Float bits, array carriers, constant/dictionary/sequence/slice/gather
views, invalid dictionary codes, sequence overflow, parent lifetime and denied
admission. Encoded framing stays with the existing codec validators.

## #757 witnesses

| Invariant | Selected witness and boundary |
| --- | --- |
| WIRE-017 | Typed materialized portal paging under concurrent insert/delete/rekey and later staged changes; existing streaming snapshot tests. |
| WIRE-018 | Existing completed INSERT RETURNING re-execution tests and selected transaction portal handoffs: no mutation replay. |
| WIRE-019 | Existing statement cascade/idempotent Close and transaction boundary cleanup tests; final source leases return to zero. |
| WIRE-020 | Existing cumulative result-row cap across portal resumes; whole failing page remains atomic. |
| WIRE-021 | Existing shared retained-memory budget, huge max_rows, Close/transaction/disconnect/cancellation lifecycle tests. Materialized typed output uses the same portal owner. |

The historical post-Sync schema-change transcript is governed by the selected
transaction-scoped portal contract: post-Sync resume reports missing portal
without replay or rows. This slice does not select a longer portal lifetime or
new transaction visibility profile.

## Red and green evidence

The encoded owner witness failed while the draft used only row cursors. It passed
after the validated retained-owner handoff. Its read-counter assertion then
failed and passed after restoring storage-read instrumentation. Full validation
also exposed five executor regressions: parallel dispatch/fallback, LIMIT, EXISTS
and result-cap behavior. All 102 executor integration tests passed after restoring
those dispatch and source-bound contracts; all 162 executor unit tests passed.
Further focused runs exposed missing collection read-path instrumentation and
case-distinct wildcard identity in a CTE. The existing metrics witness and all
222 parser/type tests passed after restoring the event and shared stored-field
identity/label handling.

The final build, complete locked tests, full pedantic clippy, format check and
validation of touched test files remain mandatory. No pending run is a pass and
this acceptance record does not promote later kernel or production-profile support.
