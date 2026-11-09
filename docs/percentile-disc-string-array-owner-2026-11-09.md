# Discrete percentile String-array result ownership

The #806 array-fraction result currently charges its JSON array slots to the
sorted-input reservation and returns a bare value after that reservation ends.
The scalar discrete-percentile SQL Array clone has a separate unqualified
handoff: even moving it with the sorted owner leaves an unchecked deep Array
clone in `OutputRow`.

Jev selected a narrower end-to-end result owner for `percentile_disc` array
fractions when all sorted non-NULL input values are Strings (0.91; gap lead
0.71), receipt `target/assembly-review/806-percentile-array-string-owner-{request,response}.json`.
Preserve fraction validation, FILTER, sort, no-row NULL and the existing
all-NULL/empty array path first. Track whether every non-NULL input is a String
when the existing input loop retains each value; sorting preserves this flag.
Any other value and all `percentile_cont` calls retain their existing path.

For the selected path preserve the original per-fraction rank operation order,
NULL fraction positions and the actual sorted String spelling. Build a private
String/null JSON array under a separate value-first reservation, admitting its
directory and each UTF-8 String before allocation and checking actual copy
chunks. Transfer that owner into `OutputRow` under a private String/null copy
policy. Validate that policy's payload and pre-admit each alias's directory and
String bytes; check actual element and UTF-8 copies. Sorted, fraction, result
and row owners overlap through use, including cancellation and error cleanup.
Allocation and control errors may precede later element processing and alias
publication; partial row values must remain owned on failure.

Test-first probes cover rank/order and differently spelled equal keys, DESC,
FILTER, NULL and empty/no-row behavior, the actual construction/copy boundaries,
foreign tracker, exact/below measured peak, aliases and last-row-clone lifetime,
forged payload rejection, and mixed/non-String/CONT fallback. Input evaluation,
comparison, numeric/nested/non-String results, scalar SQL Array cloning,
generic array cloning, allocator transients, group payloads and synchronous
teardown remain unqualified. These probes are authored and unexecuted; this
does not complete #806.
