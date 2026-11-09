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
