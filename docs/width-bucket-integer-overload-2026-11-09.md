# WIDTH_BUCKET integer operand overload

PostgreSQL 18.6 resolves three SMALLINT, INT or BIGINT operands to the FLOAT8
four-argument WIDTH_BUCKET overload. At the 2^53 precision boundary, BIGINT
operand 9007199254740993 with bounds 9007199254740992 and 9007199254740994
returns bucket 1 ascending and 3 descending. With an explicit NUMERIC operand
and integer bounds PostgreSQL instead uses the exact NUMERIC overload and
returns bucket 2. NULL, count validation and INT4 result overflow retain their
existing precedence. The oracle was stopped after these probes.

Remove the special runtime rule that sent three Int64 values to NUMERIC;
existing i64-to-f64 conversion rounds to nearest, ties to even. Explicit
NUMERIC values still select the existing numeric route. This source-determined
change does not qualify mixed NUMERIC/FLOAT8 or REAL overload selection,
numeric-to-float coercion before NULL, conversion diagnostics or #813. SQL and
wire probes are authored but unexecuted pending consolidated validation.
