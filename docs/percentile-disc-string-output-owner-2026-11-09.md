# Scalar discrete percentile String ownership

The #806 scalar `percentile_disc` path clones the selected sorted value and
returns it without a result reservation. Jev selected a String-only owner
(0.99; gap lead 0.75), receipt
`target/assembly-review/806-percentile-disc-string-{request,response}.json`.
After existing fraction validation, filtering and sorting, compute the rank
once using the existing `len().to_f64`, multiply, ceil, `to_usize`,
`saturating_sub(1)` and `min(len - 1)` order. If that selected original value
is a String, copy it into a value-first same-tracker reservation using checked
UTF-8 chunks while sorted and fraction owners remain live. The existing
`OutputRow` owns the result and each admitted alias copy; retain a zero-byte
reservation for an empty String. A foreign tracker must fail with a
percentile-specific diagnostic, without changing the existing mode diagnostic.

This moves String allocation and admission before handoff and introduces
resource/cancellation errors during its copy. Fraction validation and existing
NULL, empty, filtering, sorting and rank errors retain their ordering. The
non-String scalar and all array and continuous paths retain their existing
selection and clone behavior. Input evaluation, comparison, arbitrary selected
Values, non-null array serialization, group payloads, allocator transients and
synchronous teardown remain unqualified; this does not complete #806.

PostgreSQL 18.6 returned `a`, `b`, `c` for fractions 0, 0.5, 1 over
`[a, b, c, NULL]`, `b` at 0.5 DESC, and `a` at 0.5 with `b` filtered out.
On `[a, b, c, d]`, fractions 0.1, 0.5, 0.50001, 1 selected `a`, `b`, `c`,
`d`; an empty String remained empty, a typed NULL fraction returned NULL and
an always-false FILTER returned NULL. Untyped NULL overload resolution is a
separate issue. Cassie probes are authored but unexecuted.
