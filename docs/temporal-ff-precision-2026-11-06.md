# Formatted timestamp precision increment

This increment follows merged #1193 and records the selected FF1–FF6 precision
behavior under open #815. The initial gap was that the parser consumed only
the specified fraction width, losing digits needed for rounding. The pinned
PostgreSQL 18.6 oracle confirmed that FF3 input `.1235` must produce `.124`.

The first regression retains that exact case. The implementation and focused
regressions are present in the current source baseline; this closeout branch
requalifies them with the selected ordinary text `MSK` increment. No baseline
failure is claimed for the original probe.

The selected cases cover post-epoch and pre-epoch rounding, explicit/session
timezone conversion, carry into the next second/day, and range rejection after
rounding. The precision propagation decision and semantic evidence are recorded
below. Preserve existing MS/US input semantics and typed TIME endpoints. Keep
Midge as the direct published dependency; this work needs no storage capability
or persistent layout change.

## Selected contract before implementation

Pinned PostgreSQL 18.6 confirms complete delimited FF input is interpreted at
microsecond precision, then rounded to FF1–FF6 precision on the resulting UTC
instant. Signed pre-epoch rounding follows the existing timestamp typmod rule
(half away from the PostgreSQL epoch). Apply explicit/session timezone conversion
before rounding; validate the timestamp range before and after rounding so a
carry to the exclusive upper endpoint returns `22008`.

Jev selected preserving the existing public `parse`/`Parsed` API through a
crate-private `parse_with_precision` tuple with probability 0.98, and final UTC
rounding followed by range validation with probability 0.98. These are advisory
contract judgments, verified against source and the native semantic oracle.
Store FF precision only in the private parsing builder and tuple; do not add a
persistent representation or public struct field. Retain fixed-width parsing
when a numeric field follows, and preserve MS/US semantics and typed TIME24.

Seven focused SQL regressions were authored before production edits. They cover
FF1–FF6 precision, pre-epoch rounding, explicit and session timezone
conversion, second/day carry, range rejection, and numeric-token boundaries.

Implementation now carries private FF precision through conversion, reads full
delimited fractions with bounded microsecond values, rounds the UTC instant,
and retains range checks on both sides of rounding. The existing public Parsed
shape and parse signature are unchanged. Production format.rs remains below
1,000 lines.

The native upper-bound probe uncovered a PostgreSQL 18.6 edge discrepancy:
rounding `294276-12-31 23:59:59.9995` with FF3 emits the exclusive upper
endpoint, although later formatting that value rejects it. Cassie's proposed
post-round `22008` guard enforces its existing valid timestamp range rather
than treating that transient native endpoint as a supported value. Focused Jev
selected rejecting the rounded out-of-range instant with `22008` (probability
0.97). This explicit finite Cassie deviation preserves the exclusive
`END_TIMESTAMP` invariant. Native direct parsing of the endpoint and
`to_char` of the transient result both reject it; the scalar rounding probe
alone is not represented as native `22008` evidence.

## Closeout scope

The current #815 slice qualifies this merged FF implementation together with
the selected ordinary text `MSK` resolver. This bounded slice does not complete
the broader #815 acceptance criteria.
