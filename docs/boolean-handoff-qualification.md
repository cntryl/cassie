# Plain Boolean typed handoff qualification

This finite [#1265](https://github.com/cntryl/cassie/issues/1265) optimization follows the [primitive measurement](typed-primitive-profile.md) merged through #1266 at baseline `f857c8a35b85d8c895ca1fb2324f4c96332362ac`. It targets the observed 100,000 JSON-backed Boolean lanes and temporary Value slots in each covering-index query. It does not introduce a storage format, SQL capability, public API, kernel, runtime default or native-engine change.

## Selected boundary

Existing CBC1 Plain Boolean payloads store compact non-NULL bits mapped through logical validity. The native path decodes to an admitted nullable primitive Boolean owner, retaining raw bytes, metadata and the reservation through columns, batches, selections, slices, gathers and aliases. Final drop releases backing. Numeric native handoff is unchanged. Constant, RLE and Dictionary Boolean codecs retain their fully validated JSON/Value boundary.

Reuse current framing, checksums, manifest/type/codec/count/null/length checks and the unchanged conservative field reservation: encoded length + 64 × bounded declared decoded length + row count × max(sizeof(JSON Value), 64) + field/header overhead. Expected row domain is validated before native allocation. Header decoded length retains its current bound/manifest-equality meaning; it is not replaced by a recomputed JSON length. Unused validity bits remain rejected; unused high payload bits remain accepted. No writer policy or persisted bytes change. Missing/corrupt/stale required derived input preserves whole-attempt authoritative fallback, while resource/cancellation failures keep their existing terminal boundaries. Captured/staged source visibility and existing Boolean three-valued/numeric aggregate semantics remain authoritative.

## Comparison contract

All six existing `tier3_system_typed_primitive` workloads retain exactly the same 100k nullable BIGINT/BOOLEAN relation, SQL, independent exact SUM/COUNT/type oracle, worker1, cache-off execution, query2GiB/native2GiB/memtable512MiB profiles and standard harness. The corrected before owner is `e10068c9cf9e9e5533dbcbba8a3c1c17169bd906`; original indexed CPU evidence remains explicitly `c3fd17f8e2d9c6893766604e17985bcbef3e1b66` and its executable SHA. Those immutable identities are not relabeled as new source or runtime git metadata. The merged baseline benchmark and catalog source are byte-identical to the preserved e100 owner.

The selected after evidence requires 100,000 encoded numeric lanes plus 100,000 `encoded_boolean_lanes`, with zero Boolean JSON/Value transport in indexed Plain cases. Other sources and non-Plain fallback retain truthful classifications. Inline Value slot flow and admitted query memory are not actual allocation traffic or RSS. The same indexed TRUE query gets a separate actual-PID CPU window after setup with private collection off; profiler-overhead timing is not canonical. Before/after rates, noise or regression must be reported without an invented speedup threshold. Allocation counts/bytes remain unavailable without an actual allocation profiler.

## Evidence status

The implementation is source-reviewed, with execution deferred to the complete integrated gate. New tiny probes are authored/not run, not observed RED. Final receipts will identify exact integrated source/tree, decoder/owner/visibility/fallback probes, complete ordered local gate, six actual canonical/diagnostic cases, before/after comparison, separate indexed CPU evidence, final-head hosted checks and squash/issue readback. A conversion-path reduction does not itself prove global CPU dominance or a production speedup.
