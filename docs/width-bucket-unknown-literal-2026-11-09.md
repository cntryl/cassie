# WIDTH_BUCKET unknown operands and literal count

PostgreSQL 18.6 resolves three unknown WIDTH_BUCKET operand parameters as
FLOAT8 with a direct unknown INT4 count literal or NULL count. Repeated operand
slots remain FLOAT8; a later text cast does not undo that inference. Invalid
or out-of-range count strings fail during Parse with 22P02 or 22003. The
oracle was stopped after the probes.

Jev selected a narrow Fold increment (`target/assembly-review/width-bucket-
unknown-operands-literal-contract-*`): after declared-routine selection, require
three fresh, qualified, nonliteral unknown direct operand parameters and a
direct string or NULL count. Validate a string count through the existing
owner-coupled INT4 scalar admission before changing operand slots, then apply
FLOAT8 demands in operand order and return INT4 result metadata. Previously
reviewed binder handling converts the count literal once admitted parameter
types make the operand family known. Repeated operand slots are allowed;
resolved, prior-legacy and contextual-NULL operand states bypass the path.

The existing INT4 parser's long unchecked scan and binder diagnostic-owner
boundary remain unqualified. Other unknown literal/operand shapes, complete
error ordering and #813 remain open. Authored tests are unexecuted pending
consolidated validation.
