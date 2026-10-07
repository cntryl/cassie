# Mixed numeric COALESCE promotion

[#850](https://github.com/cntryl/cassie/issues/850) owns selected-value promotion
under the existing finite type contract. This correction changes runtime numeric
behavior without adding a logical type, public AST variant or persistent format.
Midge remains the only storage layer and pgwire remains primary.

## Selected semantics

When compatible COALESCE operands resolve to FLOAT, a selected integer is
promoted before its consumer performs arithmetic. The selected operand is still
lazy: later value expressions, errors and user casts are not demanded. Existing
FLOAT carriers retain infinity, NaN and signed zero. Explicit user FLOAT casts
continue to enforce their existing finite-value policy. An integer-only common
result retains exact integer arithmetic.

The binder retains the resolved FLOAT domain as a `CAST(NULL AS FLOAT)` fallback
in the existing variadic COALESCE expression. This is a lazy semantic no-op and
is inserted at most once across repeated scope/parameter binding. The evaluator
converts only a selected integer carrier using that retained domain; it does not
inspect or execute another value expression. This preserves numeric identity
when a typed NULL parameter or a CASE/derived result loses its runtime carrier.
The extra fallback uses existing AST/type/arity laws and introduces no SQL name.
Anchoring waits until complete parameter-free binding or the concrete parameter
OID pass. Early binding can infer a provisional FLOAT for unresolved arithmetic
parameters; retaining that provisional domain would round integer-only inputs.

## Focused witnesses

`tests/sql_queries/coalesce_promotion.rs` qualifies the precision boundary,
CTE/derived scopes, view and EXISTS rebinding, nested COALESCE and CASE consumers, lazy error-bearing
fallbacks, NULL, integer-only exactness, explicit integer cast failure,
UPDATE assignments/RETURNING and INSERT SELECT.

`tests/pgwire_extended/coalesce_promotion.rs` qualifies FLOAT descriptors and exact
text/binary values, concrete and inferred integer parameter inputs, direct
parameterized CASE and forwarded CTE/derived CASE carriers, typed-NULL FLOAT
parameter domain retention, nonfinite/signed-zero FLOAT forwarding, explicit
FLOAT cast failure, and incompatible parameter rejection on empty input. The
existing Cassie incompatible-type SQLSTATE remains `42601`.
Prepared integer-only arithmetic remains BIGINT with exact text/binary values
through direct, CTE and derived COALESCE consumers.

The generic boundary fixture uses BIGINT `9007199254740993` and FLOAT NULL. The
PostgreSQL 18.6 reference recorded in #850 returns `9007199254740992` for COALESCE
and `9007199254740991` for its subtraction by one. Current-main baseline
`4a09c41a` reproduced the defect with integer carriers before the correction.
The focused direct wire witness also reproduced exact text mismatches. Final
validation and immutable tested revision belong to the PR review record.

## Qualification boundaries

NULLIF/GREATEST/LEAST are owned by [#802](https://github.com/cntryl/cassie/issues/802)
and integrate in this coherent scalar bundle. A combined precision-boundary
witness qualifies arithmetic inside those conditional arguments, SUM and
FIRST_VALUE. [#794](https://github.com/cntryl/cassie/issues/794) supplies compact
function-argument operator parsing and must merge before this bundle publishes.
The current operator candidate baseline `b1039b6b` independently reproduced all
four SQL regressions and four of eight wire witnesses before the scalar fix;
the integrated candidate passed all four SQL and eight wire witnesses.

This record does not qualify the complete dialect corpus, all physical kernels,
stronger transaction isolation or production resource/restart profiles.
