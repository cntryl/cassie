# Scalar window input row-size counting

Status: candidate contract for review. No executable validation is claimed.

Jev selected reuse of the existing controlled row-entry counter (0.98). Its
0.88 lead requires source and focused parity, cancellation, admission, overflow
and non-window probes; the receipt is in ignored
`target/assembly-review/806-window-row-accounting-*`.

Scalar window setup flattens input batches, then `batch_rows_bytes` obtains each
row's entry size by allocating `serde_json::to_vec(row.entries())` before
reserving the window row-size amount. Long string, JSON and array values can
therefore be serialized without a window-owned buffer admission or a
cooperative check during that serialization.

The proposed slice reuses the existing aggregate entry-size counter for each
row, checks interruption between rows, then adds the existing row-type estimate
and reserves the same window accounting amount. The counter has selected
checked string/JSON/array/NUMERIC traversal and preserves its legacy serializer
fallback for unsupported shapes. Numeric/JSON counting can require separately
admitted transient workspace, so cancellation or memory denial may now precede
the window reservation. Arithmetic overflow must fail as a resource error
rather than silently wrap. Per-row serialization failure must retain the prior
zero-length contribution where the existing counter explicitly reproduces it.

This does not establish bounded flattening, row-type deduplication/recursion,
unsupported fallback, partition accounting, allocation-transient excess, or
output teardown. The change does not qualify every window function or complete
#806. NUMERIC counting retains the controlled decimal formatter's documented
quadratic maximum-width CPU and BigInt allocation limitations. Test-first cases
must compare the prior byte count across supported and
fallback shapes, exercise real string/JSON/array counting cancellation and
workspace denial, show successful output/row lifetime and error cleanup, and
preserve zero/empty/NULL and filtering order. Tests and Cargo checks remain
deferred to the consolidated gate.
