# Formatted timestamp precision increment

This next increment follows merged #1193 and owns the remaining FF1–FF6
precision behavior under open #815. The current parser consumes only the
specified fraction width, losing digits needed for rounding. The prior pinned
PostgreSQL 18.6 oracle confirmed that FF3 input `.1235` must produce `.124`.

The authored first regression retains that exact case. It has not been
executed; no observed baseline failure is claimed. Compilation and test
execution remain deferred until the coherent bundle is assembled.

Before implementation, confirm FF1–FF6 behavior with the pinned native oracle
for post-epoch and pre-epoch instants, explicit/session timezone conversion,
carry into the next second/day, and range rejection after rounding. Submit the
concrete precision propagation options and source evidence to Jev and record
the selected contract here. Preserve existing MS/US input semantics and typed
TIME endpoints. Keep Midge as the direct published dependency; this work needs
no storage capability or persistent layout change.

Status at initial handoff preparation: regression authored; later contract and implementation evidence is recorded below.

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

Seven focused SQL regressions were authored before production edits and remain
unexecuted pending assembler qualification. The coder runs no Cargo gate and
makes no RED/GREEN claim from the authored probes.

Implementation now carries private FF precision through conversion, reads full
delimited fractions with bounded microsecond values, rounds the UTC instant,
and retains range checks on both sides of rounding. The existing public Parsed
shape and parse signature are unchanged. Production format.rs remains below
1,000 lines. Source formatting and diff checks passed; all seven authored
regressions, build, complete tests and strict Clippy remain unexecuted by the
coder and are handed to the assembler's single coherent-bundle gate.

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
