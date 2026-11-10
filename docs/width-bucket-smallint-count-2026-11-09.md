# WIDTH_BUCKET inference with SMALLINT count

PostgreSQL 18.6 permits SMALLINT as a four-argument WIDTH_BUCKET count via
implicit widening to INT4. Uniform NUMERIC anchors select NUMERIC unknown
operands; with no known operand anchor, unknown operands become FLOAT8. A
supplied SMALLINT parameter retains its OID21 descriptor. An operand also used
as a SMALLINT-cast count produces 42P08, with SMALLINT versus FLOAT8 detail;
negative counts report 2201G, subject to strict NULL handling. The oracle was
stopped after the probes.

Jev selected accepting qualified known OID21 counts in the existing mixed
parameter-inference gate (`target/assembly-review/width-bucket-smallint-count-
contract-*`). The count itself is not coerced or rewritten: it retains its
input type and existing Value::Int64 carrier. Existing occurrence conflict
handling, routine precedence, freshness checks and prior fallback paths apply.
Other overload gaps and #813 remain open. Authored tests are unexecuted pending
consolidated validation.
