# Controlled NUMERIC text candidate

This partial helper converts a borrowed BigInt coefficient to decimal through
admitted base-10^19 limbs, checks conversion and output work, and returns the
rendered String with its query reservation. The input owner must remain live
through conversion. Its target is the existing `Numeric` Display text,
including exact fractional scale, zero, sign and special values. Source review
and differential probes are required for each consumer; this helper does not
by itself qualify NUMERIC parsing, ordering or any output handoff.

The decimal-digit capacity uses the checked upper bound based on BigInt's bit
count. Limb, output and their overlap are reserved before allocation, with
String/Vec fields released before their reservations. `try_reserve_exact` may
transiently allocate more than requested; the helper rejects excess after
observing it and cannot claim the brief allocator excess was pre-admitted.
Public Numeric values may contain scales beyond SQL's accepted range; the
helper adds no precision or scale cap, but can reject a requested allocation
against the existing query budget.

Repeated base conversion is quadratic in coefficient width. The correctness
probes cover representative carries, scale, signs, public-scale budget and
synchronized interruption, but no maximum-width execution or performance
claim has been made. The 131072-digit SQL boundary requires a separately
designed performance/benchmark investigation; do not add timing assertions to
correctness tests. Earlier interim Jev and source reviews are recorded in the
coordinator's `target/assembly-review/tie-*` and related NUMERIC receipts.
All authored probes remain unexecuted until the consolidated validation gate.
