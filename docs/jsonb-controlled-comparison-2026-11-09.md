# Controlled scalar JSONB comparison candidate

This #809 candidate applies only when the existing scalar JSONB comparison
receives statement controls. It parses complete left and right documents in
that order with the existing owned Value-mode arena, retaining both arenas
through a checked iterative comparison. Semantic parse or numeric-format
failure returns to the existing comparator so its invalid-input behavior is
preserved; control failures propagate. Calls without controls, generic array
comparison and unrelated scalar operators retain their current routes.

The selected semantics match `json_order`: root empty arrays precede all
other roots; node ranks are null, string, number, boolean, array, object;
arrays compare length before children; objects compare member count before
keys sorted by length then UTF-8 bytes. Corresponding keys themselves compare
lexicographically by bytes before their values. Numbers use exact finite
decimal value, including signs, zero and differing scale; strings compare UTF-8
bytes. Value-mode private sentinel dispatch is intentionally preserved and its
PostgreSQL divergence/storage migration remains open.

Jev selected this scope (0.94), with ordering/object-key (0.68), numeric/error
laziness (0.75), sentinel/full-parse/NULL/fallback (0.78), and arena/stack/
scratch ownership (0.77) leads. Receipt:
`target/assembly-review/jsonb-comparison-arena-contract-{request,response}.json`.
Before implementation author differential and PostgreSQL witnesses for all
ranks, root/nested empty arrays, key-length-versus-key-comparison, duplicates,
sentinels, unsigned/numeric boundaries, malformed/unvisited numeric, NULL,
actual move/comparison cancellation, and exact/below-budget release. Arena
construction, fallback reparsing, allocator transients, source input copies,
third-party teardown and the separate storage migration remain unqualified;
these probes remain unexecuted until the combined gate.

A PostgreSQL 18.6 oracle independently returned true for the selected root
empty-array ordering, `{"z":0} > {"aa":0}`, `[-1] < [0,0]`, `1.00 = 1`,
and duplicate-last object witnesses. These narrow results do not qualify the
full comparator. The checked scalar route falls back to the legacy comparator
on an arena or numeric semantic failure, so that path reparses without bounded
controls. Query-budget or interruption errors can now precede the legacy
comparison result. Native SQL-array operands retain the existing dispatch.
