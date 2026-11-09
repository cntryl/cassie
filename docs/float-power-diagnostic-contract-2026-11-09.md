# FLOAT8 power diagnostic contract

Status: this narrow pre-release contract is selected for #813. The issue
remains open until its complete finite numeric profile is implemented and
qualified.

## Selected behavior

For the FLOAT8 `power(left, right)` function and `^` operator, preserve the
existing domain checks and calculation. When both inputs are finite, report a
finite-result overflow as SQLSTATE `22003` with message `value out of range:
overflow`; report underflow to zero from a nonzero base as SQLSTATE `22003`
with message `value out of range: underflow`. NULL, NaN, infinity, zero-negative
power, negative-base fractional power, argument coercion and result descriptors
retain their existing behavior. Exact NUMERIC power and NUMERIC-to-FLOAT8
conversion are separate paths and are not changed here.

## Embedded error surface

Cassie has not been released, and no release contract establishes
`CassieError::FloatInput(FloatInputError::Range)` as the permanent embedded
error shape for power-expression range errors. For this selected pre-release
slice, those errors intentionally become
`CassieError::TypeInput(TypeInputError { sqlstate: "22003", ... })` so both
PostgreSQL diagnostics can be represented. Other FLOAT8 input conversions
continue to use `CassieError::FloatInput`.

This is a scoped, observable embedded API change. Record it before integrating
runtime code; do not generalize it to other FLOAT8 errors. The issue remains
open for the rest of its acceptance profile.

## Planning and protocol requirements

Constant folding must propagate these two new `22003` power diagnostics at the
same planning boundary as the previous power range error. The folder sees only
`QueryError`, and the same SQLSTATE/messages are already produced by other
FLOAT4/FLOAT8 math paths. Matching by text would silently change those paths as
well. Give the two power failures typed provenance with
`QueryError::FloatPowerOverflow` and `QueryError::FloatPowerUnderflow`, and
propagate only those variants through the folding classifier. The variants are
re-exported from the public `executor` module; their addition is an intentional
pre-release API change selected to preserve the distinct power provenance.
`From<QueryError> for CassieError` maps them to the selected `TypeInput` shape.
Audit every exhaustive `QueryError` match. In particular,
`subquery_eval::is_interruption` must keep these range failures classified as
ordinary errors, not interruptions, matching the previous `FloatInput` path.
`numeric_controlled_text::format_integer` also converts `QueryError` directly;
map the new variants there without changing its existing `General`-to-
`Execution` behavior.

Regression coverage must keep constant-folding order and laziness intact, and
verify the embedded error shape, function/operator SQLSTATE and message, Parse
parameter OIDs, result OIDs, NULL and special-float behavior, and protocol
recovery after Sync. Include a focused non-interruption classification probe
and verify that unrelated supported values, coercions, and error precedence
remain unchanged. The embedded error and public `QueryError` changes are part
of qualification, not waived by this decision record.

The candidate branch before this decision changed the shared kernel to
`TypeInput` but did not classify that error as a folding error. Adding a text-
only `TypeInput` match would also affect the existing FLOAT4/FLOAT8 math paths.
Its prepared wire test asserted the result descriptor but not the parameter
OIDs. Independent source review found these gaps. Those tests and all later
clone and numeric-power obligations remain separate from this slice.

## Decision evidence

Jev selected the pre-release `TypeInput` behavior with probability `0.97`
(choice confidence `0.96`), and then selected typed error provenance with
probability `0.64` (choice confidence `0.52`). It assessed the candidate as-is
as safe with probability `0.04` and text-only folding classification as safe
with probability `0.07`. The three request/response pairs are preserved under
ignored `target/assembly-review/813-float-power-diagnostic-contract-*`,
`813-float-power-diagnostic-contract-followup-*`, and
`813-float-power-diagnostic-folding-*`.
Jev is a decision aid; source review and the deferred executable gate still
determine whether the implementation is acceptable.

The relevant source paths are `src/executor/filter/math.rs`,
`src/executor/execution/constant_planning.rs`, `src/app/error.rs`, and
`tests/pgwire_extended/functions/math/`. Builds and tests remain deferred to the
coherent consolidated bundle.
