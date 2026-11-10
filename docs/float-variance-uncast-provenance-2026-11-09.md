# Uncast FLOAT variance provenance

The explicit-cast #806 increment uses PostgreSQL's FLOAT8/REAL transition only
when the original single aggregate argument is a FLOAT8 or REAL cast. Uncast
column references still use the earlier Welford transition. PostgreSQL 18.6
returns 22003 on finite FLOAT8 overflow and differs in ordinary final bits; the
explicit-cast contract records the oracle witnesses. Uncast variance therefore
remains an issue-level gap, including grouped and window calls.

The existing bound statement and logical plan retain expressions, but no
per-expression static type/owner identity. Dynamic first-value selection is
insufficient: an early overflow can precede a later different carrier, and
worker partials run before the late row-order fallback. Row metadata alone is
not a proof of every expression result's carrier.

Binder `ResultTypes::proven_float8` is a useful existing source-provenance
analysis. It tracks FLOAT8 declarations and does not promote schema-only
inherited fields, but its `numeric_outputs::expressions::column_type` currently
uses first matching field rather than proving uniqueness. It also does not
establish REAL provenance. A blind schema-to-cast rewrite can select an
ambiguous name or change malformed-carrier coercion and error timing.

An actual same-type cast is one possible carrier handoff, not an established
contract. A projection-only rewrite changes the aggregate signature, while
aggregate collection separately walks SELECT, HAVING and ORDER BY. Identical
calls in those phases could then use different transitions or fail to resolve
the same stored result. Repeated binder validation, DISTINCT and aggregate-local
ORDER BY structural matching, visible declared routines, scoped/quoted aliases,
parameter rebind, persistent definitions, admission and diagnostic ordering
must also be accounted for. The cast must be evaluated, not merely used as a
forgeable marker. A broad public AST or plan change needs its own compatibility
decision and durable-reader proof.

The next Cassie-side design should either propagate an owned, trustworthy
type-and-origin fact coherently across every aggregate occurrence and lowering
phase, or prove a narrower whole-statement rewrite with deterministic behavior
for excluded shapes. It must select serial execution before workers when the
new transition can fail, preserve numeric/special and routine behavior, and
include exact PG result/error, alias, modifier, ownership, cancellation and
resource probes. This is a source/design boundary, not a missing Midge feature.
The previously authored Cassie probes and all builds remain unexecuted.
