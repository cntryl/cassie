# Relational semantics qualification

This finite qualification slice tracks [#761](https://github.com/cntryl/cassie/issues/761)
and the confirmed correlated SELECT output defect
[#864](https://github.com/cntryl/cassie/issues/864). Qualification is in progress;
the selected public grammar, type identities and support levels remain the
contracts in [the SQL dialect profile](sql-dialect-profile.md).

## Selected correlated output repair

Classify each `EXISTS` occurrence independently using the existing binder's
inner/outer scope authority. A successful bind without outer fields remains
uncorrelated and is folded before source execution. When that bind fails but
binding the same statement against the admitted enclosing source fields
succeeds, only that occurrence is deferred to the actual outer row. If both
binds fail, preserve the original error. This classification performs no
subquery execution and uses the same CTE scope, parameters and session context.

Uncorrelated siblings still fold before source execution, including when the
outer input is empty or another sibling is correlated. Deferred SELECT output
occurrences resolve per actual row after filtering, grouping, windows and
ordering, before projection, DISTINCT and final pagination. This remains a
scalar expression boundary and does not promote aliases or correlation to a
native kernel. Fresh AST, field, lookup and row copies must be admitted before
allocation and retain the existing parent/query/operator owners through their
handoff. Cancellation and errors release temporary owners. The source attachment
copy has its own pre-allocation lease even while the original outer row remains
live. Deferred correlated output uses the existing scalar evaluator's lazy CASE
and COALESCE selection through a private borrowed resolver; only reached nodes
execute, and conditions/arguments are evaluated once. Static uncorrelated
folding, including dead-branch and empty-input errors, remains unchanged.

Qualified references reserve their actual parsed namespaces during private
carrier allocation, including nested scopes. Alias lowering preserves an
inner carrier only when the enclosing admitted namespace would collide with
the underlying collection qualifier. An exact binder-supplied outer field
may pass alias lowering only when no inner namespace claims its qualifier.

The slice preserves existing uncorrelated and WHERE behavior. CASE, supported
function wrappers, inner shadowing, CTE scope, empty input, grouped output,
errors and owner retirement require concrete controls. HAVING, ORDER BY and
join ON behavior is not silently moved to a different evaluation phase; any
unsupported sibling discovered by qualification receives an explicit finding.

## Evidence status

The 14 seed tables have passed actual PostgreSQL-wire parameter OID and stored
carrier readback, including NULL arrays, negative zero and integers above
2^53. Literal query rows and descriptors are separate from implementation
helpers. All 22 primary literal cases and their Statement Describe/text and
binary Portal descriptors now pass focused qualification. The confirmed
three-shape correlated SELECT defect has actual red/green evidence, with
WHERE, inner shadow, CTE, CASE/function and eager empty-input/sibling controls.
The original failed attempts and preparation corrections remain retained;
this is not a completed qualification or a full-suite pass. Independent copy
ownership, threshold-minus-one denial, cancellation and final-reader retirement
now pass focused controls, including warmed lookups with spare vector capacity.
Selected CASE/COALESCE dead branches, reached errors, dynamic Boolean parameters
and a session-local volatile CASE operand pass; scalar callbacks run only for
reached nodes and preserve the independently parsed function-body error boundary.
Developer quality and a coherent implementation checkpoint remain separate from
completion of the remaining qualification matrix and full validation.

Completion still requires all 22 primary invariants, concrete positive and
negative variants, pagination parameters, rich keys, frames, operator path
witnesses, selected PostgreSQL 18 oracle records, and normal required local
and exact-head hosted validation.
