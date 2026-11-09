# `pow` alias compatibility investigation

Status: design only. No `pow` support or runtime change is introduced here.

PostgreSQL 18.6 provides both `pow` and `power` for NUMERIC and FLOAT8. Cassie's
registered scalar metadata and floating dispatcher recognize `power`; the
numeric dispatcher mentions `pow` after a registry lookup that currently cannot
resolve it. Registering the unqualified alias changes existing routine-name
collision and execution behavior, so that change is not yet safe.

Jev's first gate selected a full alias only after routine-preserving proof
(0.64, gap 0.90). A second gate preferred investigating a qualified-only
`pg_catalog.pow` increment (0.67, gap 0.57). Receipts are in ignored
`target/assembly-review/pow-{udf,qualified}-compatibility-*`. Neither decision
establishes safe activation or excludes the unqualified alias from #813.

Normal SQL CREATE rejects system-schema targets, but that is not an invariant
for every existing catalog record. `Cassie::catalog` and `Catalog::register_function`
are public. The public `Midge::put_function` canonicalizes a two-part system
name as `pg_catalog.pow`, and hydration registers persisted metadata. A
three-part `db.pg_catalog.pow` can also be visible under that schema alias in
enforced database scope. Unscoped and scoped function maps use different rules.
These existing representations must be preserved through resolution and restart.

An attempted qualified-only design would need all of the following before
runtime activation:

- Preserve declared qualified routine precedence and its existing wrong-arity
  behavior in binder validation, parameter Fold, execution and all session
  function-map paths. The binder currently checks builtin arity before its
  user-function fallback; executor function evaluation also tries builtins first.
- Avoid folding a pre-existing user routine as immutable POWER. Both execution
  constant planning and scalar constant resolution inspect builtin metadata.
  A name-only exemption would require a proven replacement for the builtin's
  early type/error behavior and must not silently change unrelated expressions.
- Preserve historical definition readback. `definition_guard::Policy` uses
  global builtin recognition without catalog origin to reject modifiers in
  stored expressions. Adding global recognition can therefore change the
  acceptance of existing persisted function bodies or other definitions.
- Preserve virtual/catalog identity, builtin metadata and index validation,
  scoped versus unscoped lookup, case handling, and direct public catalog or
  storage callers. The registry is also consulted by CREATE FUNCTION and
  scalar-index validation.
- Reproduce Parse/Bind typing and error ordering for FLOAT8, NUMERIC, unknown
  parameters/literals, NULL and invalid arguments. Pinned PostgreSQL emits the
  fully qualified signature and HINT for invalid `pg_catalog.pow` overloads;
  current `power_inputs` hardcodes `power` and its generic diagnostic does not
  establish that parity. Inherited POWER arithmetic, allocation and cancellation
  limitations stay separately open.

Required tests include existing and persisted qualified UDFs (including wrong
arity and non-immutable behavior), restart hydration, public Catalog injection,
constant and nonconstant operands, SQL CREATE/lookup with unqualified `pow`,
metadata and qualified operator outcomes, NULL/invalid-literal precedence,
text/binary descriptors and controlled resource/error cleanup. The oracle
observations are not a Cassie build or test run. Until source, contract and
executable evidence establish the above, both alias variants remain pending.
