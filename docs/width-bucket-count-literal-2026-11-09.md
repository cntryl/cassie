# WIDTH_BUCKET count literal subset

PostgreSQL 18.6 coerces an unknown fourth WIDTH_BUCKET argument to INT4 at
analysis time. An invalid literal reports 22P02 and an out-of-range literal
reports 22003 before a NULL operand can short-circuit execution. Accepted INT4
input includes signs, ASCII whitespace, radix prefixes and permitted digit
separators. The oracle was stopped after probing representative cases.

Jev selected a narrowly typed literal path (receipt
`target/assembly-review/width-bucket-literal-count-contract-*`). For builtin
four-argument calls with uniformly FLOAT8 or uniformly NUMERIC first-three
operands, a direct unknown string count uses the existing INT4 input law. The
parameter Fold reports an invalid input through its existing owner-coupled
scalar diagnostic on the wire; it otherwise preserves its previous summary and
parameter behavior. Binder validates the literal and replaces a valid literal
with the equivalent INT4 integer expression for execution. Visible declared
routines bypass the new path as they did in the preceding count checks.

The INT4 parser's existing unchecked scan on a long input and the binder's
existing Execution diagnostic allocation/lifetime remain unqualified. Early
cancellation or memory failure during admission can precede semantic errors.
All-unknown/mixed operands, additional overloads and full #813 remain open.
SQL and wire probes are authored but unexecuted pending consolidated gates.
