# Conditional expressions

The finite [#802](https://github.com/cntryl/cassie/issues/802) contract adds
`NULLIF`, `GREATEST` and `LEAST` through existing function-call syntax and existing
logical types. It introduces no storage encoding, wire identity or public AST
variant. Midge remains the direct storage layer and pgwire remains primary.

The public Rust registry adds `FunctionId::NullIf`, `FunctionId::Greatest`,
`FunctionId::Least` and `FunctionReturnType::FirstComparedArgument`. Downstream
exhaustive matches on these enums must handle the added variants. The crate and
GitVersion next-version are aligned to the approved 0.2.0 compatibility boundary;
this change does not create a release tag or publish a crate.

## Selected behavior

`NULLIF` takes exactly two comparable arguments. Equal non-NULL arguments yield
SQL NULL; otherwise it returns the first argument. SMALLINT/INT/BIGINT comparisons
remain exact, including values above 2^53. Cross-width integer comparisons retain
the first argument's declared result width. An integer/FLOAT comparison promotes
both operands to FLOAT before equality and returns FLOAT. That promotion can
round large integers: `NULLIF(CAST(9007199254740993 AS BIGINT),
CAST(9007199254740992 AS FLOAT))` returns NULL.

`GREATEST` and `LEAST` require at least one argument. Arguments resolve to one
common admitted type: integer widths widen as necessary, FLOAT participation
promotes numeric arguments, and homogeneous TEXT or BOOLEAN arguments retain
their family. They ignore SQL NULL arguments and return SQL NULL when every
argument is NULL. An entirely unknown NULL argument list resolves to TEXT.
TEXT uses Cassie's existing text ordering contract; BOOLEAN orders false before
true. Existing literal typing remains authoritative; this slice does not add
implicit conversion of TEXT literals to numeric values.

Runtime arguments are evaluated eagerly, including the second `NULLIF` argument
when the first row-column value is NULL. Error-bearing later arguments can fail
the query. PostgreSQL planner constant folding is a separate behavior and does
not promise runtime laziness. Existing `COALESCE` remains lazy.

Binding rejects incompatible known families even when the source contains no
rows. Conditional operand coercion uses resolved source scopes, including
qualified join fields, dependent CTE bodies, correlated EXISTS and supported
mutation value, predicate and RETURNING expressions. One private normalization
pass runs after the complete initial statement bind. Recursive source binding,
correlated subquery compilation and parameter revalidation add no casts.
Binding owns an outer FLOAT
cast on every numeric operand of a FLOAT-result conditional call. Its private
conditional argument evaluator normalizes integer carriers, including nested
COALESCE and derived FLOAT results, while preserving already-FLOAT carriers and
permitted nonfinite query values. Inner expressions and explicit user casts use
their existing evaluation and error rules. A NULL FLOAT operand therefore
retains its declared type during scalar evaluation. Explicitly
cast bound inputs retain their selected parameter identities; result descriptors
follow the resolved conditional result type in text and binary wire formats.

## Boundaries

The admitted families are SMALLINT, INT, BIGINT, FLOAT, TEXT and BOOLEAN. Temporal,
CHAR/VARCHAR, ARRAY, JSON, VECTOR and new logical families are excluded.
Schema-qualified conditional names, VARIADIC arguments, unconstrained parameter
signature inference and additional PostgreSQL implicit-cast rules are not
qualified by this slice. Existing query cancellation, memory and result limits
still apply; adding these expressions does not qualify new operator kernels or
promote their enclosing Experimental query families.

The coherent bundle also corrects mixed-numeric COALESCE promotion tracked in
[#850](https://github.com/cntryl/cassie/issues/850). Its selected value enters the
resolved FLOAT domain before inner arithmetic; conditional normalization then
uses that correctly evaluated value. Combined precision-boundary witnesses cover
direct conditional, aggregate and window consumers with compact operator suffixes.

## Evidence

Generic acceptance fixtures live in
`tests/sql_queries/conditional_expressions.rs` and
`tests/pgwire_extended/conditional_expressions.rs`. They cover NULLs, exact
integer comparisons, FLOAT promotion, eager dynamic errors, common descriptors,
explicit parameter casts, empty-input validation and existing aggregate/window,
join, CTE and RETURNING boundaries. Wire fixtures compare statement and portal
descriptors with executed text and binary DataRows.

The independent semantic oracle was PostgreSQL 18.6 Homebrew on aarch64,
compiled by Apple clang 21.0.0 (`clang-2100.1.1.101`), with postgres executable
SHA256 `db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.
Reduced local fixtures established integer first-width descriptors, FLOAT
promotion, all-NULL TEXT, Boolean/text ordering, incompatible empty-input binding
and dynamic argument errors. Constant-NULL and NULL-column probes distinguished
planner simplification from runtime argument demand.

The current typed pipeline is qualified separately under
[#756](https://github.com/cntryl/cassie/issues/756). Function projections retain
existing query dispatch under its [finite contract](typed-pipeline-acceptance.md).
The focused private conditional fixture exercises an initially bound expression
through `TypedBatch::project` and its bounded scalar adapter, checking FLOAT
carriers, output metadata and memory release after a native predicate. This does
not establish a native conditional kernel or enable a new query dispatch route.
