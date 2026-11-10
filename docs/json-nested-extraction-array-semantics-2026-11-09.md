# Multidimensional string-backed extraction paths

The `#>` and `#>>` string-backed fallback parsed a PostgreSQL TEXT[] literal
then iterated `ParsedTextArray.value`, whose top-level values are nested arrays
for a multidimensional brace literal. It interpreted the first nested array as
a NULL path component instead of visiting the row-major scalar elements. The
same fully validated parse already supplies `flat_values`; using that field for
this fallback follows the existing native `SqlArray` behavior without altering
deletion, JSON document parsing, path index rules, SQL NULL or output formatting.
One-dimensional and JSON-array input retain the same normalized elements.

PostgreSQL 18.6 returned `7` for
`'{"a":{"b":7}}'::jsonb #> '{{a,b}}'` and the explicitly typed version,
including `[0:0][3:4]={{a,b}}`. The SQL expression binder normally casts an
unknown literal to native TEXT[], but `evaluate_user_defined_function` parses a
stored function body at execution and evaluates it without that binder rewrite.
The authored SQL function witness exercises that string-backed route. Direct
`Expr::Param` evaluator coverage establishes the internal scalar behavior only;
public embedded parameter admission may reject an explicitly TEXT-typed value
and is not claimed. Document validation remains before path parsing. The legacy
fallback's allocations, interruption and error string ownership remain
unqualified; the separate ordinary-sentinel round-trip semantics remains open.
All tests are unexecuted, and source/PG observations do not complete #809.
