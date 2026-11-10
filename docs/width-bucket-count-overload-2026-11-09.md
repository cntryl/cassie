# WIDTH_BUCKET count overload subset

PostgreSQL 18.6 resolves four-argument `width_bucket` with an INT4 count;
SMALLINT widens to INT4, while typed BIGINT, NUMERIC and FLOAT8 counts have no
matching overload and fail with 42883 during Parse, even if an operand is NULL.
An unknown parameter resolves to INT4; an untyped string is coerced to INT4.
The oracle was stopped after the probes.

Jev selected a limited binder check (receipt
`target/assembly-review/width-bucket-count-binder-contract-*`). For builtin
four-argument calls, when all first three inferred argument types are SmallInt,
Int, BigInt, Float, extended Real or Numeric, and the count type is BigInt,
Float, extended Real or Numeric, use the existing undefined-function
error before evaluation. This preserves the current wider behavior for
unresolved or other argument types and the two-argument array route. A visible
declared routine of the same name disables this check, including when its arity
differs: `numeric_function_type` currently treats such a routine as overriding
math builtins because `has_distinct_builtin_arity` does not cover them. This
is a conservative preservation of existing Cassie behavior, not a complete
PostgreSQL routine-resolution claim.

Unknown parameter inference, unknown-string coercion and errors, other typed
arguments, all other arities and routine-resolution parity remain open. Tests
are authored but unexecuted under the consolidated validation deferral; #813
is incomplete.

## Qualification amendment (2026-11-10)

The prior conservative rule for a visible routine with a different arity is
superseded for `width_bucket`. The unqualified legacy path permits a
user-defined routine named `width_bucket` when its declared arity differs from
the four-argument builtin. Calls matching the UDF arity select that routine;
four-argument calls select the builtin and retain its input checks. Existing
search-path-scoped function identity resolution remains unchanged. This is a
narrow exception and does not establish general user-function overload
resolution or same-arity builtin/UDF overloads.

PostgreSQL 18.6 confirms that `CREATE FUNCTION public.width_bucket(x integer)`
succeeds, a four-argument call with an INT4 count resolves to the builtin,
BIGINT count has no matching function (42883), and an invalid unknown count
fails INT4 input coercion (22P02), even while the one-argument routine is
visible. The corresponding Cassie regressions must assert those exact
distinctions. Jev selected this bounded contract (confidence 0.91); the
request and response are retained under ignored `target/jev/pr1195-width-bucket-contract/`.

This amendment does not complete #813. General numeric overload qualification,
other `width_bucket` argument families, and the remaining issue acceptance
criteria stay open.
